//! Custom Windows Named Pipe listener with proper security descriptors.
//!
//! The default `ssh-agent-lib::NamedPipeListener::bind()` creates pipes with
//! no security descriptor (`NULL` `SECURITY_ATTRIBUTES`), which causes
//! Windows OpenSSH `ssh.exe` / `ssh-add.exe` to get `ACCESS_DENIED` when
//! connecting to `\\.\pipe\openssh-ssh-agent`.
//!
//! This module creates the pipe with a security descriptor based on the
//! official Windows OpenSSH ssh-agent (from `PowerShell/openssh-portable`
//! `contrib/win32/win32compat/ssh-agent/agent.c`):
//!
//! ```text
//! D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)(A;;GA;;;<current-user-SID>)
//! ```
//!
//! - `SY` (SYSTEM): Full Control — for service interop
//! - `BA` (Built-in Administrators): Full Control
//! - `AU` (Authenticated Users): `FILE_GENERIC_READ | FILE_GENERIC_WRITE`
//!   minus `FILE_CREATE_PIPE_INSTANCE` (`0x12019b`) — clients can connect
//!   but cannot create rogue server instances
//! - Current user SID: Full Control (`GA`) — allows us to create additional
//!   pipe instances in the accept loop
//!
//! The OpenSSH agent runs as `LocalSystem` (which matches `SY`) so it can
//! use just `0x12019b` for `AU`. We run as a regular user, so we need an
//! explicit ACE for our own SID to create subsequent pipe instances.

use std::ffi::OsString;
use std::io;
use std::ptr;

use ssh_agent_lib::async_trait;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// Base SDDL template — the current user's SID is appended at runtime.
///
/// `{SID}` is replaced with the SID string of the current process user.
///
/// Reference: `PowerShell/openssh-portable` `agent.c` line ~100:
///   `sddl_str = L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)";`
const PIPE_SDDL_TEMPLATE: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)(A;;GA;;;{SID})";

const SDDL_REVISION_1: u32 = 1;
const TOKEN_QUERY: u32 = 0x0008;

// ---- Win32 FFI declarations ----

unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string_sd: *const u16,
        string_sd_revision: u32,
        sd: *mut *mut core::ffi::c_void,
        sd_size: *mut u32,
    ) -> i32;

    fn LocalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;

    fn GetCurrentProcess() -> isize;

    fn OpenProcessToken(
        process_handle: isize,
        desired_access: u32,
        token_handle: *mut isize,
    ) -> i32;

    fn GetTokenInformation(
        token_handle: isize,
        token_information_class: u32,
        token_information: *mut u8,
        token_information_length: u32,
        return_length: *mut u32,
    ) -> i32;

    fn ConvertSidToStringSidW(
        sid: *const u8,
        string_sid: *mut *mut u16,
    ) -> i32;

    fn CloseHandle(handle: isize) -> i32;
}

/// `TOKEN_INFORMATION_CLASS::TokenUser` = 1
const TOKEN_USER_CLASS: u32 = 1;

// ---- SDDL construction ----

/// Get the SID string (e.g. `S-1-5-21-...`) of the current process user.
fn current_user_sid_string() -> io::Result<String> {
    unsafe {
        // Open the current process token.
        let mut token: isize = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }

        // Query TOKEN_USER size.
        let mut needed: u32 = 0;
        GetTokenInformation(token, TOKEN_USER_CLASS, ptr::null_mut(), 0, &mut needed);
        // Expected to fail with ERROR_INSUFFICIENT_BUFFER.
        if needed == 0 {
            CloseHandle(token);
            return Err(io::Error::last_os_error());
        }

        // Allocate and query TOKEN_USER.
        let mut buf = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TOKEN_USER_CLASS,
            buf.as_mut_ptr(),
            needed,
            &mut needed,
        ) == 0
        {
            CloseHandle(token);
            return Err(io::Error::last_os_error());
        }
        CloseHandle(token);

        // TOKEN_USER is { SID_AND_ATTRIBUTES { PSID, DWORD } }.
        // On 64-bit, PSID is at offset 0 (8 bytes), DWORD at offset 8.
        // We only need the PSID (first pointer-sized field).
        let psid = *(buf.as_ptr() as *const *const u8);

        // Convert SID to string.
        let mut sid_str_ptr: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(psid, &mut sid_str_ptr) == 0 {
            return Err(io::Error::last_os_error());
        }

        // Read the wide string.
        let mut len = 0;
        while *sid_str_ptr.add(len) != 0 {
            len += 1;
        }
        let sid_string = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str_ptr, len));
        LocalFree(sid_str_ptr as *mut core::ffi::c_void);
        Ok(sid_string)
    }
}

/// Build the SDDL string with the current user's SID.
fn build_pipe_sddl() -> io::Result<String> {
    let sid = current_user_sid_string()?;
    Ok(PIPE_SDDL_TEMPLATE.replace("{SID}", &sid))
}

// ---- Security descriptor wrapper ----

/// RAII wrapper around a security descriptor allocated by
/// `ConvertStringSecurityDescriptorToSecurityDescriptorW`.
struct SecurityDescriptor(*mut core::ffi::c_void);

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: *mut core::ffi::c_void = ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

/// Win32 `SECURITY_ATTRIBUTES` layout — must match the C struct exactly.
#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut core::ffi::c_void,
    b_inherit_handle: i32,
}

// ---- Pipe creation ----

/// Create a `NamedPipeServer` with the proper security descriptor.
///
/// `first_instance`: if `true`, sets `FILE_FLAG_FIRST_PIPE_INSTANCE` so the
/// call fails if another server already holds this pipe name.
fn create_pipe(name: &OsString, sddl: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    let sd = SecurityDescriptor::from_sddl(sddl)?;
    let mut sa = SecurityAttributes {
        n_length: std::mem::size_of::<SecurityAttributes>() as u32,
        lp_security_descriptor: sd.0,
        b_inherit_handle: 0, // FALSE
    };
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first_instance);
    // SAFETY: `sa` points to a valid SECURITY_ATTRIBUTES whose lifetime
    // exceeds this synchronous call, and whose `lpSecurityDescriptor` was
    // allocated by `ConvertStringSecurityDescriptorToSecurityDescriptorW`.
    unsafe {
        opts.create_with_security_attributes_raw(
            name,
            &mut sa as *mut SecurityAttributes as *mut core::ffi::c_void,
        )
    }
}

// ---- Listener ----

/// Named pipe listener with proper Windows security descriptors.
///
/// Drop-in replacement for `ssh_agent_lib::agent::NamedPipeListener` that
/// creates pipes with an SDDL modeled on the official OpenSSH ssh-agent,
/// plus an ACE granting the current user full control for instance creation.
#[derive(Debug)]
pub struct SecureNamedPipeListener {
    current: NamedPipeServer,
    name: OsString,
    sddl: String,
}

impl SecureNamedPipeListener {
    /// Bind to a named pipe, creating the first instance with the
    /// OpenSSH-compatible security descriptor.
    pub fn bind(pipe: impl Into<OsString>) -> io::Result<Self> {
        let name = pipe.into();
        let sddl = build_pipe_sddl()?;
        log::debug!("Pipe SDDL: {sddl}");
        let current = create_pipe(&name, &sddl, true)?;
        Ok(Self {
            current,
            name,
            sddl,
        })
    }
}

#[async_trait]
impl ssh_agent_lib::agent::ListeningSocket for SecureNamedPipeListener {
    type Stream = NamedPipeServer;

    async fn accept(&mut self) -> io::Result<Self::Stream> {
        // Wait for a client to connect to the current pipe instance.
        self.current.connect().await?;
        // Create a fresh pipe instance for the next client (NOT first_instance).
        let next = create_pipe(&self.name, &self.sddl, false)?;
        // Swap: return the connected pipe, keep the fresh one for next accept.
        Ok(std::mem::replace(&mut self.current, next))
    }
}

// ---- Client PID extraction ----

unsafe extern "system" {
    fn GetNamedPipeClientProcessId(pipe: isize, client_process_id: *mut u32) -> i32;
}

/// Extract the client process ID from a connected `NamedPipeServer`.
fn get_client_pid(pipe: &NamedPipeServer) -> u32 {
    use std::os::windows::io::AsRawHandle;
    let handle = pipe.as_raw_handle() as isize;
    let mut pid: u32 = 0;
    let ok = unsafe { GetNamedPipeClientProcessId(handle, &mut pid) };
    if ok == 0 {
        log::warn!(
            "GetNamedPipeClientProcessId failed: {}",
            io::Error::last_os_error()
        );
        0
    } else {
        pid
    }
}

/// `Agent` impl for our `SshAgentHandler` so `ssh_agent_lib::agent::listen()`
/// works with our custom listener.
impl<U: crate::UiCallback> ssh_agent_lib::agent::Agent<SecureNamedPipeListener>
    for crate::ssh_agent::SshAgentHandler<U>
{
    fn new_session(&mut self, socket: &NamedPipeServer) -> impl ssh_agent_lib::agent::Session {
        let pid = get_client_pid(socket);
        log::debug!("New session from client PID: {pid}");
        self.with_client_pid(pid)
    }
}
