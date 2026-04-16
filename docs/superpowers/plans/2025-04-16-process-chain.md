# Process Chain: Structured SSH Client Identification

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current single-process SSH client identification (always shows `ssh.exe`) with a structured process chain that walks the process tree to reveal the true initiator (e.g., `git.exe → ssh.exe`) and captures command-line arguments at each level.

**Architecture:** Add a `ProcessInfo` struct and `resolve_process_chain()` function that uses Win32 APIs (`CreateToolhelp32Snapshot`, `NtQueryInformationProcess` + `ReadProcessMemory`) to walk parent processes, stopping at known shells. The chain is stored alongside existing `client_exe`/`client_pid` fields (backward compat), serialized as JSON in SQLite, and rendered in the frontend as a chain with hover tooltips.

**Tech Stack:** Rust (Win32 FFI, no new crates), TypeScript/Solid.js, SQLite

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `crates/bw-agent/src/process.rs` | **Create** | New module: `ProcessInfo` struct, `resolve_process_chain()` with platform-specific impls |
| `crates/bw-agent/src/ssh_agent.rs` | Modify | Use `resolve_process_chain()` instead of `resolve_client_exe()`, pass chain to approval + log |
| `crates/bw-agent/src/approval.rs` | Modify | Add `process_chain: Vec<ProcessInfo>` to `ApprovalRequest` |
| `crates/bw-agent/src/access_log.rs` | Modify | Add `process_chain` column (JSON TEXT), schema migration |
| `crates/bw-agent/src/lib.rs` | Modify | Add `pub mod process;`, re-export `ProcessInfo` |
| `src-tauri/src/main.rs` | Modify | Update notification body to use process chain |
| `src/lib/tauri.ts` | Modify | Add `ProcessInfo` type, update `ApprovalRequest` and `AccessLogEntry` |
| `src/components/ApprovalDialog.tsx` | Modify | Render process chain with tooltips |
| `src/components/LogTable.tsx` | Modify | Render process chain in log entries |

---

## Chunk 1: Rust Core — ProcessInfo + Process Tree Walking

### Task 1: Create `ProcessInfo` struct and module skeleton

**Files:**
- Create: `crates/bw-agent/src/process.rs`
- Modify: `crates/bw-agent/src/lib.rs`

- [ ] **Step 1: Create `process.rs` with `ProcessInfo` struct and `resolve_process_chain` signature**

```rust
// crates/bw-agent/src/process.rs

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

/// Known shell / terminal process names (lowercase). Walking stops when we hit one of these.
const SHELL_STOP_LIST: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "bash.exe",
    "bash",
    "zsh",
    "fish",
    "sh",
    "explorer.exe",
    "windowsterminal.exe",
    "wt.exe",
    "conhost.exe",
    "sshd",
    "sshd.exe",
    "systemd",
    "init",
    "launchd",
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
        return vec![ProcessInfo {
            exe: "unknown".to_string(),
            pid: 0,
            cmdline: "unknown".to_string(),
        }];
    }

    let direct = query_process_info(pid);

    // Walk parent chain.
    let mut chain = vec![direct.clone()];
    let mut current_pid = pid;

    for _ in 0..MAX_WALK_DEPTH {
        let parent_pid = get_parent_pid(current_pid);
        if parent_pid == 0 || parent_pid == current_pid {
            break;
        }

        let parent_info = query_process_info(parent_pid);

        // Check stop condition: is parent a known shell/terminal?
        let exe_lower = parent_info
            .exe
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&parent_info.exe)
            .to_lowercase();
        if SHELL_STOP_LIST.contains(&exe_lower.as_str()) {
            break;
        }

        chain.push(parent_info);
        current_pid = parent_pid;
    }

    // Reverse: topmost initiator first, direct client last.
    chain.reverse();
    chain
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
    use std::ptr;

    // PROCESSENTRY32W size = 568 bytes on 64-bit
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
    // Parse /proc/<pid>/status for PPid field.
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

#[cfg(windows)]
fn resolve_cmdline(pid: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_VM_READ: u32 = 0x0010;

    // ProcessBasicInformation = 0
    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: usize,       // ExitStatus (NTSTATUS, padded to pointer size)
        peb_base_address: usize,
        reserved2: [usize; 2],  // AffinityMask, BasePriority
        unique_process_id: usize,
        reserved3: usize,       // InheritedFromUniqueProcessId
    }

    // RTL_USER_PROCESS_PARAMETERS offsets we care about:
    // On 64-bit: CommandLine (UNICODE_STRING) is at offset 0x70.
    // UNICODE_STRING: { Length: u16, MaximumLength: u16, _pad: u32, Buffer: *mut u16 }
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

    // NtQueryInformationProcess from ntdll.dll
    type NtQueryInformationProcessFn = unsafe extern "system" fn(
        process_handle: isize,
        process_information_class: u32,
        process_information: *mut u8,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> i32;

    unsafe {
        // Load NtQueryInformationProcess dynamically.
        let ntdll = windows_sys_load_ntdll();
        if ntdll.is_null() {
            return "unknown".to_string();
        }
        let nqip = get_nqip(ntdll);
        if nqip.is_none() {
            return "unknown".to_string();
        }
        let nqip = nqip.unwrap();

        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
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
        // UNICODE_STRING: Length (u16) + MaximumLength (u16) + padding (u32) + Buffer (usize)
        let mut cmdline_struct = [0u8; 16]; // enough for UNICODE_STRING on 64-bit
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
        // Buffer pointer is at offset 8 on 64-bit (after Length u16 + MaxLength u16 + pad u32).
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
unsafe fn windows_sys_load_ntdll() -> *mut core::ffi::c_void {
    unsafe extern "system" {
        fn GetModuleHandleW(module_name: *const u16) -> *mut core::ffi::c_void;
    }
    // "ntdll.dll" in UTF-16
    let name: [u16; 10] = [b'n' as u16, b't' as u16, b'd' as u16, b'l' as u16, b'l' as u16,
                            b'.' as u16, b'd' as u16, b'l' as u16, b'l' as u16, 0];
    unsafe { GetModuleHandleW(name.as_ptr()) }
}

/// Get NtQueryInformationProcess function pointer.
#[cfg(windows)]
unsafe fn get_nqip(ntdll: *mut core::ffi::c_void) -> Option<NtQueryInformationProcessFn> {
    unsafe extern "system" {
        fn GetProcAddress(module: *mut core::ffi::c_void, name: *const u8) -> *mut core::ffi::c_void;
    }
    let func = unsafe {
        GetProcAddress(ntdll, b"NtQueryInformationProcess\0".as_ptr())
    };
    if func.is_null() {
        None
    } else {
        Some(std::mem::transmute(func))
    }
}

// NtQueryInformationProcessFn type alias needs to be module-level for the unsafe fn.
#[cfg(windows)]
type NtQueryInformationProcessFn = unsafe extern "system" fn(
    process_handle: isize,
    process_information_class: u32,
    process_information: *mut u8,
    process_information_length: u32,
    return_length: *mut u32,
) -> i32;

#[cfg(unix)]
fn resolve_cmdline(pid: u32) -> String {
    // /proc/<pid>/cmdline uses NUL as separator.
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
    fn test_shell_stop_list_contains_common_shells() {
        assert!(SHELL_STOP_LIST.contains(&"cmd.exe"));
        assert!(SHELL_STOP_LIST.contains(&"powershell.exe"));
        assert!(SHELL_STOP_LIST.contains(&"bash"));
        assert!(SHELL_STOP_LIST.contains(&"explorer.exe"));
    }
}
```

- [ ] **Step 2: Register module in `lib.rs`**

In `crates/bw-agent/src/lib.rs`, add:

```rust
pub mod process;
```

after the existing `pub mod approval;` line. Also add re-export:

```rust
pub use process::ProcessInfo;
```

- [ ] **Step 3: Build and run tests**

Run: `cargo build -p bw-agent && cargo test -p bw-agent -- process`
Expected: Build succeeds, 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/bw-agent/src/process.rs crates/bw-agent/src/lib.rs
git commit -m "feat: add process chain module with tree walking and cmdline capture"
```

---

## Chunk 2: Wire Process Chain into Approval + Access Log

### Task 2: Update `ApprovalRequest` to include process chain

**Files:**
- Modify: `crates/bw-agent/src/approval.rs`

- [ ] **Step 1: Add `process_chain` field to `ApprovalRequest`**

Add import at top:

```rust
use crate::process::ProcessInfo;
```

Add field to `ApprovalRequest` struct (after `client_pid`):

```rust
pub process_chain: Vec<ProcessInfo>,
```

- [ ] **Step 2: Update `create_request` to accept and store process chain**

Change signature to:

```rust
pub async fn create_request(
    &self,
    key_name: &str,
    fingerprint: &str,
    client_exe: &str,
    pid: u32,
    process_chain: Vec<ProcessInfo>,
) -> (ApprovalRequest, oneshot::Receiver<bool>) {
```

Add in the `ApprovalRequest` construction:

```rust
process_chain,
```

- [ ] **Step 3: Update tests in `approval.rs`**

All 3 test calls to `create_request` need the new parameter. Add `vec![]` as the last argument:

```rust
// e.g.:
let (request, rx) = queue
    .create_request("ssh-ed25519", "SHA256:abc", "ssh.exe", 1234, vec![])
    .await;
```

- [ ] **Step 4: Build and test**

Run: `cargo test -p bw-agent -- approval`
Expected: 3 approval tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/bw-agent/src/approval.rs
git commit -m "feat: add process_chain field to ApprovalRequest"
```

---

### Task 3: Update `AccessLog` to store process chain

**Files:**
- Modify: `crates/bw-agent/src/access_log.rs`

- [ ] **Step 1: Add `process_chain` field to `AccessLogEntry`**

Add import:

```rust
use crate::process::ProcessInfo;
```

Add field to `AccessLogEntry` (after `client_pid`):

```rust
pub process_chain: Vec<ProcessInfo>,
```

- [ ] **Step 2: Update schema with migration**

Replace the `init_schema` method body with:

```rust
fn init_schema(&self) -> rusqlite::Result<()> {
    let conn = self.conn.lock().expect("lock poisoned");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS access_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
            key_fingerprint TEXT NOT NULL,
            key_name TEXT NOT NULL,
            client_exe TEXT NOT NULL,
            client_pid INTEGER NOT NULL,
            approved INTEGER NOT NULL,
            process_chain TEXT NOT NULL DEFAULT '[]'
        )",
    )?;

    // Migration: add column if table already exists without it.
    // ALTER TABLE ADD COLUMN is a no-op if column already exists in SQLite 3.35+,
    // but we catch the error for older versions.
    let _ = conn.execute_batch(
        "ALTER TABLE access_log ADD COLUMN process_chain TEXT NOT NULL DEFAULT '[]'"
    );

    Ok(())
}
```

- [ ] **Step 3: Update `record()` to accept and store process chain**

Change signature:

```rust
pub fn record(
    &self,
    fingerprint: &str,
    key_name: &str,
    exe: &str,
    pid: u32,
    approved: bool,
    process_chain: &[ProcessInfo],
) -> rusqlite::Result<()> {
```

Update the INSERT:

```rust
let chain_json = serde_json::to_string(process_chain).unwrap_or_else(|_| "[]".to_string());
conn.execute(
    "INSERT INTO access_log (key_fingerprint, key_name, client_exe, client_pid, approved, process_chain) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    rusqlite::params![fingerprint, key_name, exe, pid, approved as i32, chain_json],
)?;
```

- [ ] **Step 4: Update `query()` to read process chain**

In the `query_map` closure, add after `approved`:

```rust
process_chain: {
    let json_str: String = row.get(7)?;
    serde_json::from_str(&json_str).unwrap_or_default()
},
```

Note: column index 7 (0-based: id=0, timestamp=1, key_fingerprint=2, key_name=3, client_exe=4, client_pid=5, approved=6, process_chain=7).

Update the SELECT to include the new column:

```sql
SELECT id, timestamp, key_fingerprint, key_name, client_exe, client_pid, approved, process_chain FROM access_log ORDER BY id DESC LIMIT ?1
```

- [ ] **Step 5: Update tests**

Update `test_log_and_query`:

```rust
#[test]
fn test_log_and_query() {
    use crate::process::ProcessInfo;

    let log = AccessLog::open_in_memory().unwrap();
    log.record("SHA256:abc", "my-key", "ssh.exe", 1234, true, &[
        ProcessInfo { exe: "git.exe".to_string(), pid: 1200, cmdline: "git push".to_string() },
        ProcessInfo { exe: "ssh.exe".to_string(), pid: 1234, cmdline: "ssh git@github.com".to_string() },
    ]).unwrap();
    log.record("SHA256:def", "other-key", "git.exe", 5678, false, &[]).unwrap();
    let entries = log.query(10).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key_fingerprint, "SHA256:def"); // most recent first
    assert!(entries[1].approved);
    assert_eq!(entries[1].process_chain.len(), 2);
    assert_eq!(entries[1].process_chain[0].exe, "git.exe");
}
```

- [ ] **Step 6: Build and test**

Run: `cargo test -p bw-agent -- access_log`
Expected: Test passes.

- [ ] **Step 7: Commit**

```bash
git add crates/bw-agent/src/access_log.rs
git commit -m "feat: store process_chain in access log (JSON column)"
```

---

### Task 4: Wire process chain through `ssh_agent.rs` sign flow

**Files:**
- Modify: `crates/bw-agent/src/ssh_agent.rs`

- [ ] **Step 1: Replace `resolve_client_exe` with process chain**

Remove the entire `resolve_client_exe` function (lines 76-125).

In the `sign()` method, replace:

```rust
// Resolve client executable path from PID.
let client_exe = resolve_client_exe(self.client_pid);
```

with:

```rust
// Resolve full process chain from client PID.
let process_chain = crate::process::resolve_process_chain(self.client_pid);
// client_exe = topmost initiator (first in chain).
let client_exe = process_chain
    .first()
    .map(|p| p.exe.clone())
    .unwrap_or_else(|| "unknown".to_string());
```

- [ ] **Step 2: Pass process chain to `create_request`**

Update the call:

```rust
let (approval_request, approval_rx) = self
    .approval_queue
    .create_request(&key_name, &fingerprint_str, &client_exe, self.client_pid, process_chain.clone())
    .await;
```

- [ ] **Step 3: Pass process chain to `access_log.record`**

Update the call:

```rust
if let Err(e) = self.access_log.record(
    &fingerprint_str,
    &key_name,
    &client_exe,
    self.client_pid,
    approved,
    &process_chain,
) {
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p bw-agent && cargo test -p bw-agent`
Expected: All tests pass. The `test_request_identities_returns_empty_when_no_entries` test should still pass (it doesn't call `sign`).

- [ ] **Step 5: Commit**

```bash
git add crates/bw-agent/src/ssh_agent.rs
git commit -m "feat: wire process chain through SSH sign flow"
```

---

### Task 5: Update notification in `main.rs`

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Update `send_approval_notification` to show process chain**

Replace the `send_approval_notification` function body:

```rust
fn send_approval_notification(app_handle: &tauri::AppHandle, request: &bw_agent::ApprovalRequest) {
    use tauri_plugin_notification::NotificationExt;

    let chain_display = if request.process_chain.is_empty() {
        request.client_exe.clone()
    } else {
        request
            .process_chain
            .iter()
            .map(|p| {
                p.exe
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&p.exe)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" → ")
    };

    let body = format!(
        "{} requests access to key \"{}\"",
        chain_display, request.key_name,
    );

    if let Err(error) = app_handle
        .notification()
        .builder()
        .title("SSH Key Access Requested")
        .body(body)
        .show()
    {
        log::warn!("Failed to send notification: {error}");
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p bw-agent-tauri` (or the workspace: `cargo build`)
Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat: show process chain in system notification"
```

---

## Chunk 3: Frontend — Types + UI

### Task 6: Update TypeScript types

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add `ProcessInfo` type and update interfaces**

Add after the `Config` interface:

```typescript
export interface ProcessInfo {
  exe: string;
  pid: number;
  cmdline: string;
}
```

Add `process_chain` field to `ApprovalRequest` (after `timestamp`):

```typescript
process_chain: ProcessInfo[];
```

Add `process_chain` field to `AccessLogEntry` (after `approved`):

```typescript
process_chain: ProcessInfo[];
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: add ProcessInfo type to frontend"
```

---

### Task 7: Update `ApprovalDialog.tsx` to render process chain

**Files:**
- Modify: `src/components/ApprovalDialog.tsx`

- [ ] **Step 1: Replace Client/PID display with process chain**

Import `For` from solid-js (add to existing import), and `ProcessInfo` from tauri.

Replace the "Client" and "PID" `<div>` blocks (lines 62-70) with a process chain display:

```tsx
<div class="flex justify-between items-start">
  <span class="font-medium text-gray-500">Process:</span>
  <div class="text-right">
    <Show
      when={req().process_chain.length > 0}
      fallback={
        <span class="font-semibold text-gray-900" title={req().client_exe}>
          {extractExeName(req().client_exe)} (PID: {req().client_pid})
        </span>
      }
    >
      <div class="flex items-center gap-1 flex-wrap justify-end">
        <For each={req().process_chain}>
          {(proc, index) => (
            <>
              <Show when={index() > 0}>
                <span class="text-gray-400 text-xs">→</span>
              </Show>
              <span
                class="font-semibold text-gray-900 cursor-default"
                title={`${proc.exe}\nPID: ${proc.pid}\n${proc.cmdline}`}
              >
                {extractExeName(proc.exe)}
              </span>
            </>
          )}
        </For>
      </div>
    </Show>
  </div>
</div>
```

Also add a "Command" row to display the last process's cmdline (ssh target info):

```tsx
<Show when={req().process_chain.length > 0}>
  <div class="flex justify-between items-start">
    <span class="font-medium text-gray-500">Target:</span>
    <span class="font-mono text-xs text-gray-900 truncate max-w-[200px]" title={req().process_chain[req().process_chain.length - 1].cmdline}>
      {req().process_chain[req().process_chain.length - 1].cmdline}
    </span>
  </div>
</Show>
```

- [ ] **Step 2: Build frontend**

Run: `pnpm build`
Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add src/components/ApprovalDialog.tsx
git commit -m "feat: render process chain in approval dialog"
```

---

### Task 8: Update `LogTable.tsx` to render process chain

**Files:**
- Modify: `src/components/LogTable.tsx`

- [ ] **Step 1: Replace Client column with process chain display**

Import `For`, `Show` from solid-js (update existing import).

Replace the Client `<td>` content (line 59-60) with:

```tsx
<td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
  <Show
    when={log.process_chain && log.process_chain.length > 0}
    fallback={<div title={log.client_exe}>{extractExeName(log.client_exe)}</div>}
  >
    <div class="flex items-center gap-1" title={log.process_chain.map(p => `${p.exe} (${p.cmdline})`).join('\n')}>
      <For each={log.process_chain}>
        {(proc, index) => (
          <>
            <Show when={index() > 0}>
              <span class="text-gray-400 text-xs">→</span>
            </Show>
            <span>{extractExeName(proc.exe)}</span>
          </>
        )}
      </For>
    </div>
  </Show>
</td>
```

- [ ] **Step 2: Build frontend**

Run: `pnpm build`
Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add src/components/LogTable.tsx
git commit -m "feat: render process chain in access log table"
```

---

## Chunk 4: Full Integration Build + Verify

### Task 9: Full build and final verification

- [ ] **Step 1: Run full Rust test suite**

Run: `cargo test -p bw-agent`
Expected: All tests pass.

- [ ] **Step 2: Run full frontend build**

Run: `pnpm build`
Expected: Build succeeds.

- [ ] **Step 3: Run full workspace build (Tauri)**

Run: `cargo build`
Expected: Build succeeds with no errors.

- [ ] **Step 4: Verify LSP diagnostics clean on all changed files**

Check diagnostics for:
- `crates/bw-agent/src/process.rs`
- `crates/bw-agent/src/ssh_agent.rs`
- `crates/bw-agent/src/approval.rs`
- `crates/bw-agent/src/access_log.rs`
- `crates/bw-agent/src/lib.rs`
- `src-tauri/src/main.rs`
- `src/lib/tauri.ts`
- `src/components/ApprovalDialog.tsx`
- `src/components/LogTable.tsx`

Expected: No errors.
