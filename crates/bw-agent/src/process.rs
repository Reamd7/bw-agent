/// Information about a single process in the chain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessInfo {
    /// Full executable path, or "unknown" if unavailable.
    pub exe: String,
    /// Process ID.
    pub pid: u32,
    /// Full command line, or "unknown" if unavailable.
    pub cmdline: String,
}

/// Hard-stop processes: true root/system processes where walking must stop.
/// These are never useful as SSH initiators and indicate we've reached the
/// top of the meaningful process tree.
const HARD_STOP_LIST: &[&str] = &[
    "explorer.exe",
    "windowsterminal.exe",
    "wt.exe",
    "conhost.exe",
    "sshd",
    "sshd.exe",
    "systemd",
    "init",
    "launchd",
    "services.exe",
    "svchost.exe",
    "wininit.exe",
    "csrss.exe",
    "smss.exe",
];

/// Transparent processes: included in the chain but NOT shown in the final
/// display. These are common "wrapper" processes that obscure the real
/// initiator (e.g., `cmd.exe /c "git pull"` spawned by Node.js `exec()`).
/// The walk continues through them.
const TRANSPARENT_LIST: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "bash.exe",
    "bash",
    "zsh",
    "fish",
    "sh",
];

/// Maximum number of levels to walk (prevents infinite loops from PID reuse).
const MAX_WALK_DEPTH: usize = 10;

/// Best-effort resolve of a client PID to a structured process chain.
///
/// Returns a `Vec<ProcessInfo>` ordered from the **topmost initiator** (e.g. `git.exe`)
/// down to the direct pipe client (e.g. `ssh.exe`).
///
/// If walking fails at any point, returns a chain containing only the direct client.
pub fn resolve_process_chain(pid: u32) -> Vec<ProcessInfo> {
    if pid == 0 {
        log::debug!("resolve_process_chain: pid=0, returning unknown");
        return vec![ProcessInfo {
            exe: "unknown".to_string(),
            pid: 0,
            cmdline: "unknown".to_string(),
        }];
    }

    let direct = query_process_info(pid);
    log::debug!(
        "resolve_process_chain: direct client pid={pid} exe={} cmdline={}",
        direct.exe,
        direct.cmdline
    );

    // Walk parent chain.
    let mut chain = vec![direct];
    let mut current_pid = pid;

    for depth in 0..MAX_WALK_DEPTH {
        let parent_pid = get_parent_pid(current_pid);
        log::debug!(
            "resolve_process_chain: depth={depth} current_pid={current_pid} -> parent_pid={parent_pid}"
        );
        if parent_pid == 0 || parent_pid == current_pid {
            log::debug!("resolve_process_chain: stopping (parent_pid={parent_pid})");
            break;
        }

        let parent_info = query_process_info(parent_pid);
        log::debug!(
            "resolve_process_chain: parent pid={} exe={} cmdline={}",
            parent_info.pid,
            parent_info.exe,
            parent_info.cmdline
        );

        let exe_lower = parent_info
            .exe
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&parent_info.exe)
            .to_lowercase();

        // Hard stop: system/root processes — stop walking, don't include.
        if HARD_STOP_LIST.contains(&exe_lower.as_str()) {
            log::debug!("resolve_process_chain: hard stop at system process: {exe_lower}");
            break;
        }

        // Transparent: wrapper shells — include in chain but keep walking.
        if TRANSPARENT_LIST.contains(&exe_lower.as_str()) {
            log::debug!(
                "resolve_process_chain: transparent process, continuing through: {exe_lower}"
            );
        }

        chain.push(parent_info);
        current_pid = parent_pid;
    }

    log::debug!(
        "resolve_process_chain: raw chain ({} entries): {}",
        chain.len(),
        chain
            .iter()
            .map(|p| format!(
                "{}({})",
                p.exe.rsplit(['/', '\\']).next().unwrap_or(&p.exe),
                p.pid
            ))
            .collect::<Vec<_>>()
            .join(" -> ")
    );

    // Reverse: topmost initiator first, direct client last.
    chain.reverse();

    // Filter out transparent wrapper processes from the display chain,
    // but keep at least the direct client (last element).
    let filtered: Vec<ProcessInfo> = chain
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            let is_last = *i == chain.len() - 1;
            if is_last {
                return true; // always keep the direct client (ssh.exe)
            }
            let name = p
                .exe
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&p.exe)
                .to_lowercase();
            !TRANSPARENT_LIST.contains(&name.as_str())
        })
        .map(|(_, p)| p.clone())
        .collect();

    log::debug!(
        "resolve_process_chain: filtered chain ({} entries): {}",
        filtered.len(),
        filtered
            .iter()
            .map(|p| format!(
                "{}({})",
                p.exe.rsplit(['/', '\\']).next().unwrap_or(&p.exe),
                p.pid
            ))
            .collect::<Vec<_>>()
            .join(" -> ")
    );

    filtered
}

/// Query process info (exe path + command line) for a given PID.
fn query_process_info(pid: u32) -> ProcessInfo {
    let exe = resolve_exe(pid);
    let cmdline = resolve_cmdline(pid);
    ProcessInfo { exe, pid, cmdline }
}

// ---- Platform-specific implementations ----

#[cfg(windows)]
fn resolve_exe(pid: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return format!("pid:{pid}");
        }
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) != 0 {
            CloseHandle(handle);
            let path = OsString::from_wide(&buf[..size as usize]);
            return path.to_string_lossy().into_owned();
        }
        CloseHandle(handle);
        format!("pid:{pid}")
    }
}

#[cfg(unix)]
fn resolve_exe(pid: u32) -> String {
    if let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return exe.to_string_lossy().into_owned();
    }
    format!("pid:{pid}")
}

#[cfg(windows)]
fn get_parent_pid(pid: u32) -> u32 {
    use std::mem;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    const TH32CS_SNAPPROCESS: u32 = 0x00000002;

    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> isize;
        fn Process32FirstW(snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == -1 {
            return 0;
        }

        let mut entry: ProcessEntry32W = mem::zeroed();
        entry.dw_size = mem::size_of::<ProcessEntry32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32_process_id == pid {
                    CloseHandle(snapshot);
                    return entry.th32_parent_process_id;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
        0
    }
}

#[cfg(unix)]
fn get_parent_pid(pid: u32) -> u32 {
    let status_path = format!("/proc/{pid}/status");
    if let Ok(contents) = std::fs::read_to_string(&status_path) {
        for line in contents.lines() {
            if let Some(ppid_str) = line.strip_prefix("PPid:\t") {
                if let Ok(ppid) = ppid_str.trim().parse::<u32>() {
                    return ppid;
                }
            }
        }
    }
    0
}

// ---- Command line resolution ----

#[cfg(windows)]
type NtQueryInformationProcessFn = unsafe extern "system" fn(
    process_handle: isize,
    process_information_class: u32,
    process_information: *mut u8,
    process_information_length: u32,
    return_length: *mut u32,
) -> i32;

#[cfg(windows)]
fn resolve_cmdline(pid: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_VM_READ: u32 = 0x0010;

    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: usize,
        peb_base_address: usize,
        reserved2: [usize; 2],
        unique_process_id: usize,
        reserved3: usize,
    }

    // On 64-bit: CommandLine (UNICODE_STRING) is at offset 0x70 in RTL_USER_PROCESS_PARAMETERS.
    const PARAMS_CMDLINE_OFFSET: usize = 0x70;
    // PEB: ProcessParameters pointer is at offset 0x20 on 64-bit.
    const PEB_PARAMS_OFFSET: usize = 0x20;

    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn ReadProcessMemory(
            process: isize,
            base_address: usize,
            buffer: *mut u8,
            size: usize,
            bytes_read: *mut usize,
        ) -> i32;
    }

    unsafe {
        let ntdll = load_ntdll();
        if ntdll.is_null() {
            return "unknown".to_string();
        }
        let Some(nqip) = get_nqip(ntdll) else {
            return "unknown".to_string();
        };

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle == 0 {
            return "unknown".to_string();
        }

        // 1. Get PEB address via ProcessBasicInformation.
        let mut pbi = std::mem::zeroed::<ProcessBasicInformation>();
        let mut return_length: u32 = 0;
        let status = nqip(
            handle,
            0, // ProcessBasicInformation
            &mut pbi as *mut _ as *mut u8,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut return_length,
        );
        if status != 0 {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        let peb_addr = pbi.peb_base_address;
        if peb_addr == 0 {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        // 2. Read ProcessParameters pointer from PEB.
        let mut params_ptr: usize = 0;
        if ReadProcessMemory(
            handle,
            peb_addr + PEB_PARAMS_OFFSET,
            &mut params_ptr as *mut usize as *mut u8,
            std::mem::size_of::<usize>(),
            ptr::null_mut(),
        ) == 0
        {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        // 3. Read CommandLine UNICODE_STRING from RTL_USER_PROCESS_PARAMETERS.
        // UNICODE_STRING on 64-bit: Length (u16) + MaximumLength (u16) + pad (u32) + Buffer (usize)
        let mut cmdline_struct = [0u8; 16];
        if ReadProcessMemory(
            handle,
            params_ptr + PARAMS_CMDLINE_OFFSET,
            cmdline_struct.as_mut_ptr(),
            cmdline_struct.len(),
            ptr::null_mut(),
        ) == 0
        {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        let length = u16::from_le_bytes([cmdline_struct[0], cmdline_struct[1]]) as usize;
        let buffer_ptr = usize::from_le_bytes(cmdline_struct[8..16].try_into().unwrap_or([0; 8]));

        if length == 0 || buffer_ptr == 0 || length > 32768 {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        // 4. Read the actual command line string.
        let mut cmdline_buf = vec![0u8; length];
        if ReadProcessMemory(
            handle,
            buffer_ptr,
            cmdline_buf.as_mut_ptr(),
            length,
            ptr::null_mut(),
        ) == 0
        {
            CloseHandle(handle);
            return "unknown".to_string();
        }

        CloseHandle(handle);

        // Convert UTF-16LE bytes to String.
        let wide: Vec<u16> = cmdline_buf
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        OsString::from_wide(&wide).to_string_lossy().into_owned()
    }
}

/// Load ntdll.dll handle.
#[cfg(windows)]
unsafe fn load_ntdll() -> *mut core::ffi::c_void {
    unsafe extern "system" {
        fn GetModuleHandleW(module_name: *const u16) -> *mut core::ffi::c_void;
    }
    let name: [u16; 10] = [
        b'n' as u16,
        b't' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        b'.' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        0,
    ];
    unsafe { GetModuleHandleW(name.as_ptr()) }
}

/// Get NtQueryInformationProcess function pointer from ntdll.
#[cfg(windows)]
unsafe fn get_nqip(ntdll: *mut core::ffi::c_void) -> Option<NtQueryInformationProcessFn> {
    unsafe extern "system" {
        fn GetProcAddress(
            module: *mut core::ffi::c_void,
            name: *const u8,
        ) -> *mut core::ffi::c_void;
    }
    let func = unsafe { GetProcAddress(ntdll, c"NtQueryInformationProcess".as_ptr().cast()) };
    if func.is_null() {
        None
    } else {
        Some(unsafe {
            std::mem::transmute::<*mut core::ffi::c_void, NtQueryInformationProcessFn>(func)
        })
    }
}

#[cfg(unix)]
fn resolve_cmdline(pid: u32) -> String {
    if let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) {
        let cmdline: String = bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if !cmdline.is_empty() {
            return cmdline;
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_info_serialize() {
        let info = ProcessInfo {
            exe: "C:\\Windows\\System32\\ssh.exe".to_string(),
            pid: 1234,
            cmdline: "ssh git@github.com".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("ssh.exe"));
        let deserialized: ProcessInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pid, 1234);
    }

    #[test]
    fn test_resolve_process_chain_pid_zero() {
        let chain = resolve_process_chain(0);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].exe, "unknown");
        assert_eq!(chain[0].pid, 0);
    }

    #[test]
    fn test_hard_stop_list_contains_system_processes() {
        assert!(HARD_STOP_LIST.contains(&"explorer.exe"));
        assert!(HARD_STOP_LIST.contains(&"svchost.exe"));
        assert!(HARD_STOP_LIST.contains(&"services.exe"));
    }

    #[test]
    fn test_transparent_list_contains_shell_wrappers() {
        assert!(TRANSPARENT_LIST.contains(&"cmd.exe"));
        assert!(TRANSPARENT_LIST.contains(&"powershell.exe"));
        assert!(TRANSPARENT_LIST.contains(&"bash"));
    }
}
