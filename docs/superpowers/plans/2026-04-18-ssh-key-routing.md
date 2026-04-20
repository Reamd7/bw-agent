# SSH Key Routing Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add intelligent SSH key routing to bw-agent based on Bitwarden custom field `gh-match`, allowing multi-GitHub-account users to automatically select the correct SSH key based on git repo context.

**Architecture:** Three new modules (`git_context`, `routing`, `process` enhancements) feed into the existing `ssh_agent.rs`. The routing decision happens in `request_identities()`, filtering which keys to offer. A per-session `allowed_entry_ids` cache constrains `sign()` to only accept keys that were previously routed.

**Tech Stack:** Rust, existing `ssh-agent-lib` crate, `glob` crate for pattern matching, Win32 PEB walk for process cwd (Windows), `/proc/{pid}/cwd` (Unix).

**Spec:** `docs/ssh-key-routing-spec.md`

**Worktree:** `.worktrees/ssh-key-routing` on branch `feature/ssh-key-routing`

---

## Chunk 1: Infrastructure (parallel-safe, no cross-dependencies)

### Task 1: Add `cwd` field to `ProcessInfo` + `resolve_cwd()`

**Files:**
- Modify: `crates/bw-agent/src/process.rs`
- Test: inline `#[cfg(test)]` module

- [x] **Step 1: Write failing tests for `ProcessInfo` with `cwd` and `resolve_cwd`**

Add to `process.rs` test module:

```rust
#[test]
fn test_process_info_has_cwd_field() {
    let info = ProcessInfo {
        exe: "test".to_string(),
        pid: 1,
        cmdline: "test".to_string(),
        cwd: "/home/user/project".to_string(),
    };
    assert_eq!(info.cwd, "/home/user/project");
}

#[test]
fn test_resolve_cwd_current_process() {
    // resolve_cwd on our own PID should succeed
    let cwd = resolve_cwd(std::process::id());
    assert!(!cwd.is_empty());
    assert_ne!(cwd, "unknown");
}

#[test]
fn test_resolve_cwd_invalid_pid() {
    let cwd = resolve_cwd(999999);
    assert_eq!(cwd, "");
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib -p bw-agent -- process::tests::test_process_info_has_cwd`
Expected: FAIL — struct has no `cwd` field

- [x] **Step 3: Add `cwd` field to `ProcessInfo`**

In `process.rs`, change the struct:

```rust
pub struct ProcessInfo {
    pub exe: String,
    pub pid: u32,
    pub cmdline: String,
    pub cwd: String,
}
```

Update ALL places that construct `ProcessInfo` to include `cwd`:
- `resolve_process_chain` pid=0 case: add `cwd: "unknown".to_string()`
- `query_process_info`: call `resolve_cwd(pid)` and include the result

- [x] **Step 4: Implement `resolve_cwd()`**

Add the function after `resolve_cmdline`:

```rust
/// Best-effort resolve of a process's current working directory.
/// Returns empty string on failure (never panics).
#[cfg(windows)]
pub fn resolve_cwd(pid: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_VM_READ: u32 = 0x0010;

    // CurrentDirectory is a UNICODE_STRING at offset 0x58 in RTL_USER_PROCESS_PARAMETERS on 64-bit.
    const PARAMS_CWD_OFFSET: usize = 0x58;

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
            return String::new();
        }
        let Some(nqip) = get_nqip(ntdll) else {
            return String::new();
        };

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle == 0 {
            return String::new();
        }

        // 1. Get PEB address.
        let mut pbi = std::mem::zeroed::<ProcessBasicInformation>();
        let mut return_length: u32 = 0;
        let status = nqip(
            handle,
            0,
            &mut pbi as *mut _ as *mut u8,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut return_length,
        );
        if status != 0 {
            CloseHandle(handle);
            return String::new();
        }

        let peb_addr = pbi.peb_base_address;
        if peb_addr == 0 {
            CloseHandle(handle);
            return String::new();
        }

        // 2. Read ProcessParameters pointer.
        let mut params_ptr: usize = 0;
        if ReadProcessMemory(
            handle,
            peb_addr + PEB_PARAMS_OFFSET,
            &mut params_ptr as *mut usize as *mut u8,
            std::mem::size_of::<usize>(),
            ptr::null_mut(),
        ) == 0 {
            CloseHandle(handle);
            return String::new();
        }

        // 3. Read CurrentDirectory UNICODE_STRING.
        let mut cwd_struct = [0u8; 16];
        if ReadProcessMemory(
            handle,
            params_ptr + PARAMS_CWD_OFFSET,
            cwd_struct.as_mut_ptr(),
            cwd_struct.len(),
            ptr::null_mut(),
        ) == 0 {
            CloseHandle(handle);
            return String::new();
        }

        let length = u16::from_le_bytes([cwd_struct[0], cwd_struct[1]]) as usize;
        let buffer_ptr = usize::from_le_bytes(cwd_struct[8..16].try_into().unwrap_or([0; 8]));

        if length == 0 || buffer_ptr == 0 || length > 32768 {
            CloseHandle(handle);
            return String::new();
        }

        // 4. Read the actual string.
        let mut cwd_buf = vec![0u8; length];
        if ReadProcessMemory(
            handle,
            buffer_ptr,
            cwd_buf.as_mut_ptr(),
            length,
            ptr::null_mut(),
        ) == 0 {
            CloseHandle(handle);
            return String::new();
        }

        CloseHandle(handle);

        let wide: Vec<u16> = cwd_buf
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        OsString::from_wide(&wide).to_string_lossy().into_owned()
    }
}

/// Best-effort resolve of a process's current working directory.
/// Returns empty string on failure (never panics).
#[cfg(unix)]
pub fn resolve_cwd(pid: u32) -> String {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
```

Note: `load_ntdll`, `get_nqip`, `ProcessBasicInformation`, `PEB_PARAMS_OFFSET` are already defined in the file (used by `resolve_cmdline`). The `PARAMS_CWD_OFFSET` is `0x58` for `CurrentDirectory` vs `0x70` for `CommandLine` in `RTL_USER_PROCESS_PARAMETERS`.

- [x] **Step 5: Update `query_process_info` to call `resolve_cwd`**

Change:
```rust
fn query_process_info(pid: u32) -> ProcessInfo {
    let exe = resolve_exe(pid);
    let cmdline = resolve_cmdline(pid);
    ProcessInfo { exe, pid, cmdline }
}
```
To:
```rust
fn query_process_info(pid: u32) -> ProcessInfo {
    let exe = resolve_exe(pid);
    let cmdline = resolve_cmdline(pid);
    let cwd = resolve_cwd(pid);
    ProcessInfo { exe, pid, cmdline, cwd }
}
```

- [x] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib -p bw-agent`
Expected: ALL PASS (existing 31 tests + 3 new)

- [x] **Step 7: Commit**

```bash
git add crates/bw-agent/src/process.rs
git commit -m "feat: add cwd field to ProcessInfo and resolve_cwd()"
```

---

### Task 2: Create `git_context.rs` — repo location + URL extraction + normalization

**Files:**
- Create: `crates/bw-agent/src/git_context.rs`
- Modify: `crates/bw-agent/src/lib.rs` (add `pub mod git_context;`)

- [x] **Step 1: Write failing tests for URL normalization**

Create `crates/bw-agent/src/git_context.rs`:

```rust
//! Git repository context extraction for SSH key routing.

use crate::process::ProcessInfo;
use std::path::Path;

/// Normalize a raw git remote URL to `{host}/{owner}/{repo}` format.
///
/// Returns None for non-network URLs (local paths, etc.).
pub fn normalize_remote_url(raw_url: &str) -> Option<String> {
    let url = raw_url.trim();

    // Remove trailing .git
    let url = url.strip_suffix(".git").unwrap_or(url);

    // Remove trailing /
    let url = url.trim_end_matches('/');

    // Handle ssh:// prefix
    let url = url.strip_prefix("ssh://").unwrap_or(url);

    // Handle git:// prefix
    let url = url.strip_prefix("git://").unwrap_or(url);

    // Handle https:// prefix
    let url = url.strip_prefix("https://").unwrap_or(url);

    // Handle http:// prefix
    let url = url.strip_prefix("http://").unwrap_or(url);

    // Remove username@ prefix
    let url = if let Some(at_pos) = url.find('@') {
        &url[at_pos + 1..]
    } else {
        url
    };

    // Convert SSH-style host:owner/repo to host/owner/repo
    // SSH format: github.com:owner/repo — the first : after the host separates host from path
    let url = if let Some(colon_pos) = url.find(':') {
        // Check if this looks like host:path (no slash before colon)
        let before_colon = &url[..colon_pos];
        let after_colon = &url[colon_pos + 1..];
        if !before_colon.contains('/') && !after_colon.starts_with('/') {
            format!("{}/{}", before_colon, after_colon)
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };

    // Validate: must have at least 3 segments (host/owner/repo)
    let segments: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 3 {
        return None;
    }

    // Must have a dot in host (basic "is it a hostname" check)
    if !segments[0].contains('.') {
        return None;
    }

    Some(url)
}

/// Find a git process in the chain and extract its cwd.
fn find_git_cwd(process_chain: &[ProcessInfo]) -> Option<&str> {
    for proc_info in process_chain {
        let exe_lower = proc_info
            .exe
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&proc_info.exe)
            .to_lowercase();
        if exe_lower == "git" || exe_lower == "git.exe" {
            if !proc_info.cwd.is_empty() && proc_info.cwd != "unknown" {
                return Some(&proc_info.cwd);
            }
        }
    }
    None
}

/// Locate the git config file from a working directory.
///
/// Handles:
/// - Normal repos: `.git/config`
/// - Worktrees: `.git` file → read pointer → real config
/// - Subdirectories: walk up to find `.git`
/// - Bare repos: `config` at root (with `HEAD` present)
fn find_git_config(start_dir: &Path) -> Option<std::path::PathBuf> {
    let mut dir = start_dir.to_path_buf();

    for _ in 0..10 {
        let git_path = dir.join(".git");

        if git_path.is_dir() {
            // Normal repo
            let config = git_path.join("config");
            if config.exists() {
                return Some(config);
            }
        } else if git_path.is_file() {
            // Worktree: .git is a file containing "gitdir: /path/..."
            if let Ok(content) = std::fs::read_to_string(&git_path) {
                if let Some(gitdir) = content.strip_prefix("gitdir: ") {
                    let gitdir = gitdir.trim();
                    let config = std::path::Path::new(gitdir).join("config");
                    if config.exists() {
                        return Some(config);
                    }
                    // Worktree gitdir might be relative
                    let abs_gitdir = dir.join(gitdir);
                    let config = abs_gitdir.join("config");
                    if config.exists() {
                        return Some(config);
                    }
                }
            }
        } else if dir.join("HEAD").exists() && dir.join("config").exists() {
            // Bare repo (no .git directory)
            return Some(dir.join("config"));
        }

        // Walk up
        if !dir.pop() {
            break;
        }
    }

    None
}

/// Parse a git config file and extract the URL for the given remote.
fn extract_remote_url_from_config(config_path: &Path, remote_name: &str) -> Option<String> {
    let content = std::fs::read_to_string(config_path).ok()?;

    let section_header = format!("[remote \"{}\"]", remote_name);
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_section = trimmed.starts_with(&section_header);
            continue;
        }

        if in_section {
            if let Some(url_value) = trimmed.strip_prefix("url = ") {
                let url = url_value.trim().to_string();
                if !url.is_empty() {
                    return Some(url);
                }
            }
            if let Some(url_value) = trimmed.strip_prefix("url=") {
                let url = url_value.trim().to_string();
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
    }

    None
}

/// Extract a normalized remote URL from the process chain.
///
/// This is the main entry point for git context extraction.
/// Returns None if git context cannot be determined.
pub fn extract_remote_url(process_chain: &[ProcessInfo]) -> Option<String> {
    let cwd = find_git_cwd(process_chain)?;
    let config_path = find_git_config(Path::new(cwd))?;
    let raw_url = extract_remote_url_from_config(&config_path, "origin")?;
    normalize_remote_url(&raw_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url_ssh_colon() {
        assert_eq!(
            normalize_remote_url("git@github.com:mycompany/repo.git"),
            Some("github.com/mycompany/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_ssh_scheme() {
        assert_eq!(
            normalize_remote_url("ssh://git@github.com/mycompany/repo.git"),
            Some("github.com/mycompany/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_https() {
        assert_eq!(
            normalize_remote_url("https://github.com/mycompany/repo.git"),
            Some("github.com/mycompany/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_gitlab() {
        assert_eq!(
            normalize_remote_url("git@gitlab.com:team/project.git"),
            Some("gitlab.com/team/project".to_string())
        );
    }

    #[test]
    fn test_normalize_url_no_suffix() {
        assert_eq!(
            normalize_remote_url("git@github.com:org/repo"),
            Some("github.com/org/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_local_path() {
        assert_eq!(normalize_remote_url("/local/path"), None);
    }

    #[test]
    fn test_normalize_url_file_uri() {
        assert_eq!(normalize_remote_url("file:///local/path"), None);
    }

    #[test]
    fn test_normalize_url_trailing_slash() {
        assert_eq!(
            normalize_remote_url("https://github.com/org/repo/"),
            Some("github.com/org/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_too_short() {
        assert_eq!(normalize_remote_url("github.com"), None);
        assert_eq!(normalize_remote_url("github.com/org"), None);
    }

    #[test]
    fn test_extract_remote_url_no_git_process() {
        let chain = vec![ProcessInfo {
            exe: "ssh.exe".to_string(),
            pid: 100,
            cmdline: "ssh git@github.com".to_string(),
            cwd: "/home/user".to_string(),
        }];
        assert_eq!(extract_remote_url(&chain), None);
    }

    #[test]
    fn test_extract_remote_url_git_no_cwd() {
        let chain = vec![ProcessInfo {
            exe: "git".to_string(),
            pid: 100,
            cmdline: "git push".to_string(),
            cwd: String::new(),
        }];
        assert_eq!(extract_remote_url(&chain), None);
    }

    #[test]
    fn test_extract_remote_url_from_config_parses_correctly() {
        let dir = std::env::temp_dir().join("bw-agent-test-git-config");
        std::fs::create_dir_all(&dir).unwrap();
        let config_content = r#"
[core]
    repositoryformatversion = 0
[remote "origin"]
    url = git@github.com:mycompany/repo.git
    fetch = +refs/heads/*:refs/remotes/origin/*
[branch "main"]
    remote = origin
"#;
        let config_path = dir.join("config");
        std::fs::write(&config_path, config_content).unwrap();

        let result = extract_remote_url_from_config(&config_path, "origin");
        assert_eq!(result, Some("git@github.com:mycompany/repo.git".to_string()));

        // cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [x] **Step 2: Register module in `lib.rs`**

Add `pub mod git_context;` to `crates/bw-agent/src/lib.rs` after the `pub mod config;` line.

- [x] **Step 3: Run tests**

Run: `cargo test --lib -p bw-agent`
Expected: ALL PASS

- [x] **Step 4: Commit**

```bash
git add crates/bw-agent/src/git_context.rs crates/bw-agent/src/lib.rs
git commit -m "feat: add git_context module for repo detection and URL normalization"
```

---

### Task 3: Create `routing.rs` — pattern matching + route decision

**Files:**
- Create: `crates/bw-agent/src/routing.rs`
- Modify: `crates/bw-agent/src/lib.rs` (add `pub mod routing;`)

- [x] **Step 1: Create `routing.rs` with tests**

Create `crates/bw-agent/src/routing.rs`:

```rust
//! SSH key routing based on Bitwarden custom field `gh-match`.

use bw_core::db::Entry;

/// The custom field name used for routing rules.
const MATCH_FIELD_NAME: &str = "gh-match";

/// Extract all `gh-match` patterns from an entry's custom fields.
pub fn extract_match_patterns(entry: &Entry) -> Vec<String> {
    entry
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some(MATCH_FIELD_NAME))
        .filter_map(|f| f.value.clone())
        .collect()
}

/// Check if a remote URL matches any of the given glob patterns.
///
/// Uses simple glob matching: `*` matches any sequence of characters.
/// Invalid patterns are treated as non-matching (no panic).
pub fn matches_any_pattern(remote_url: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if simple_glob_match(remote_url, pattern) {
            return true;
        }
    }
    false
}

/// Simple glob matching: only supports `*` (matches any characters).
/// No `?`, no `[]`, no `**`.
fn simple_glob_match(text: &str, pattern: &str) -> bool {
    let text_bytes = text.as_bytes();
    let pattern_bytes = pattern.as_bytes();

    // Two-pointer approach for single-level glob
    let mut ti = 0;
    let mut pi = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text_bytes.len() {
        if pi < pattern_bytes.len() && pattern_bytes[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern_bytes.len()
            && (pattern_bytes[pi] == text_bytes[ti]
                || pattern_bytes[pi] == b'?')
        {
            ti += 1;
            pi += 1;
        } else if star_pi != usize::MAX {
            // Backtrack: * consumes one more character
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    // Consume trailing *
    while pi < pattern_bytes.len() && pattern_bytes[pi] == b'*' {
        pi += 1;
    }

    pi == pattern_bytes.len()
}

/// Routing decision: categorize entries based on match rules.
#[derive(Debug, Clone)]
enum EntryCategory {
    /// Entry has no `gh-match` fields — always returned.
    Generic,
    /// Entry has `gh-match` fields that match the remote URL.
    Matched,
    /// Entry has `gh-match` fields but none match — excluded.
    Excluded,
}

/// Route entries based on remote URL and `gh-match` custom fields.
///
/// Returns the filtered list of entries that should be offered as identities.
///
/// **Fallback cascade:**
/// 1. matched + generic entries (routing hit)
/// 2. generic entries only (no routing hit, use generic keys)
/// 3. all entries (no generic keys available, full compatibility fallback)
pub fn route_entries(entries: &[Entry], remote_url: Option<&str>) -> Vec<Entry> {
    let Some(remote_url) = remote_url else {
        // No git context — return everything (full fallback)
        return entries.to_vec();
    };

    let mut matched = Vec::new();
    let mut generic = Vec::new();
    let mut _excluded_count = 0usize;

    for entry in entries {
        let patterns = extract_match_patterns(entry);
        if patterns.is_empty() {
            generic.push(entry.clone());
        } else if matches_any_pattern(remote_url, &patterns) {
            matched.push(entry.clone());
        } else {
            _excluded_count += 1;
        }
    }

    if !matched.is_empty() {
        // Matched entries + generic entries
        let mut result = matched;
        result.extend(generic);
        result
    } else if !generic.is_empty() {
        // No match, but generic keys available
        generic
    } else {
        // Nothing matched, no generic keys — return all (compatibility)
        entries.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bw_core::db::{EntryData, Field};

    fn make_ssh_entry(name: &str, fields: Vec<Field>) -> Entry {
        Entry {
            id: name.to_string(),
            org_id: None,
            folder: None,
            folder_id: None,
            name: name.to_string(),
            data: EntryData::SshKey {
                private_key: None,
                public_key: Some("fake-key".to_string()),
                fingerprint: None,
            },
            fields,
            notes: None,
            history: Vec::new(),
            key: None,
            master_password_reprompt: bw_core::api::CipherRepromptType::None,
        }
    }

    fn make_field(name: &str, value: &str) -> Field {
        Field {
            ty: Some(bw_core::api::FieldType::Text),
            name: Some(name.to_string()),
            value: Some(value.to_string()),
            linked_id: None,
        }
    }

    #[test]
    fn test_extract_match_patterns_found() {
        let entry = make_ssh_entry(
            "work",
            vec![
                make_field("gh-match", "github.com/mycompany/*"),
                make_field("other", "ignored"),
            ],
        );
        let patterns = extract_match_patterns(&entry);
        assert_eq!(patterns, vec!["github.com/mycompany/*"]);
    }

    #[test]
    fn test_extract_match_patterns_multiple() {
        let entry = make_ssh_entry(
            "work",
            vec![
                make_field("gh-match", "github.com/company/*"),
                make_field("gh-match", "github.com/company-*/*"),
            ],
        );
        let patterns = extract_match_patterns(&entry);
        assert_eq!(
            patterns,
            vec!["github.com/company/*", "github.com/company-*/*"]
        );
    }

    #[test]
    fn test_extract_match_patterns_empty() {
        let entry = make_ssh_entry("generic", vec![]);
        assert!(extract_match_patterns(&entry).is_empty());
    }

    #[test]
    fn test_glob_match_star() {
        assert!(simple_glob_match(
            "github.com/mycompany/repo",
            "github.com/mycompany/*"
        ));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(simple_glob_match(
            "github.com/mycompany/repo",
            "github.com/mycompany/repo"
        ));
    }

    #[test]
    fn test_glob_no_match() {
        assert!(!simple_glob_match(
            "github.com/other/repo",
            "github.com/mycompany/*"
        ));
    }

    #[test]
    fn test_glob_match_multiple_stars() {
        assert!(simple_glob_match(
            "github.com/mycompany-frontend/app",
            "github.com/mycompany-*/*"
        ));
    }

    #[test]
    fn test_glob_match_prefix_star() {
        assert!(simple_glob_match(
            "github.com/any-org/any-repo",
            "github.com/*/*"
        ));
    }

    #[test]
    fn test_route_entries_single_match() {
        let work = make_ssh_entry(
            "work",
            vec![make_field("gh-match", "github.com/company/*")],
        );
        let personal = make_ssh_entry(
            "personal",
            vec![make_field("gh-match", "github.com/me/*")],
        );
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work.clone(), personal.clone(), generic.clone()];
        let result = route_entries(&entries, Some("github.com/company/repo"));

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|e| e.id == "work"));
        assert!(result.iter().any(|e| e.id == "generic"));
        assert!(!result.iter().any(|e| e.id == "personal"));
    }

    #[test]
    fn test_route_entries_fallback_generic() {
        let work = make_ssh_entry(
            "work",
            vec![make_field("gh-match", "github.com/company/*")],
        );
        let personal = make_ssh_entry(
            "personal",
            vec![make_field("gh-match", "github.com/me/*")],
        );
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work, personal.clone(), generic.clone()];
        let result = route_entries(&entries, Some("github.com/unknown/repo"));

        // No match, but generic available → only generic
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "generic");
    }

    #[test]
    fn test_route_entries_fallback_all() {
        let work = make_ssh_entry(
            "work",
            vec![make_field("gh-match", "github.com/company/*")],
        );
        let personal = make_ssh_entry(
            "personal",
            vec![make_field("gh-match", "github.com/me/*")],
        );

        let entries = vec![work, personal];
        let result = route_entries(&entries, Some("github.com/unknown/repo"));

        // No match, no generic → return all
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_route_entries_no_remote_url() {
        let work = make_ssh_entry(
            "work",
            vec![make_field("gh-match", "github.com/company/*")],
        );
        let entries = vec![work];
        let result = route_entries(&entries, None);
        assert_eq!(result.len(), 1); // Full fallback
    }

    #[test]
    fn test_route_entries_excluded_not_in_generic_fallback() {
        let work = make_ssh_entry(
            "work",
            vec![make_field("gh-match", "github.com/company/*")],
        );
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work, generic.clone()];
        let result = route_entries(&entries, Some("github.com/other/repo"));

        // work is excluded (doesn't match), only generic returned
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "generic");
    }

    #[test]
    fn test_matches_any_pattern_empty_patterns() {
        assert!(!matches_any_pattern("github.com/org/repo", &[]));
    }
}
```

- [x] **Step 2: Register module in `lib.rs`**

Add `pub mod routing;` to `crates/bw-agent/src/lib.rs`.

- [x] **Step 3: Run tests**

Run: `cargo test --lib -p bw-agent`
Expected: ALL PASS

- [x] **Step 4: Commit**

```bash
git add crates/bw-agent/src/routing.rs crates/bw-agent/src/lib.rs
git commit -m "feat: add routing module for SSH key matching and filtering"
```

---

## Chunk 2: Integration

### Task 4: Integrate routing into `request_identities()` and `sign()`

**Files:**
- Modify: `crates/bw-agent/src/ssh_agent.rs`

- [x] **Step 1: Add `allowed_entry_ids` field to `SshAgentHandler`**

In `ssh_agent.rs`, add to the struct:

```rust
pub struct SshAgentHandler<U: crate::UiCallback> {
    state: Arc<Mutex<State>>,
    client: bw_core::api::Client,
    ui: Arc<U>,
    approval_queue: Arc<ApprovalQueue>,
    access_log: Arc<AccessLog>,
    client_pid: u32,
    /// Entry IDs allowed for the current session (set by request_identities).
    /// None means routing hasn't been executed yet.
    allowed_entry_ids: Option<Vec<String>>,
}
```

Update `Clone`, `new`, `with_client_pid` to handle the new field.

- [x] **Step 2: Update `request_identities()` to use routing**

Before the `for entry in &state.entries` loop, add routing logic:

```rust
// Route entries based on git context.
let process_chain = crate::process::resolve_process_chain(self.client_pid);
let remote_url = crate::git_context::extract_remote_url(&process_chain);
log::debug!("request_identities: remote_url={:?}", remote_url);

let routed_entries = crate::routing::route_entries(&state.entries, remote_url.as_deref());

// Cache allowed entry IDs for sign() enforcement.
self.allowed_entry_ids = Some(routed_entries.iter().map(|e| e.id.clone()).collect());

log::info!(
    "request_identities: routing returned {} entries (from {} total)",
    routed_entries.len(),
    state.entries.len()
);
```

Then change the loop to iterate `routed_entries` instead of `state.entries`.

- [x] **Step 3: Add per-session authorization check in `sign()`**

At the start of `sign()`, after `ensure_unlocked`, add:

```rust
// Per-session routing enforcement.
if let Some(allowed_ids) = &self.allowed_entry_ids {
    let state = self.state.lock().await;
    let mut found_id = None;
    for entry in &state.entries {
        if let bw_core::db::EntryData::SshKey {
            public_key: Some(encrypted_pubkey),
            ..
        } = &entry.data
        {
            if let Ok(pubkey_plain) = auth::decrypt_cipher(
                &state,
                encrypted_pubkey,
                entry.key.as_deref(),
                entry.org_id.as_deref(),
            ) {
                if let Ok(pubkey) =
                    ssh_agent_lib::ssh_key::PublicKey::from_openssh(&pubkey_plain)
                {
                    if let Ok(bytes) = pubkey.to_bytes() {
                        if bytes == requested_bytes {
                            found_id = Some(entry.id.clone());
                            break;
                        }
                    }
                }
            }
        }
    }
    if let Some(id) = found_id {
        if !allowed_ids.contains(&id) {
            log::warn!("sign() rejected: entry {} not in session allowed set", id);
            return Err(ssh_agent_lib::error::AgentError::Other(
                "Sign request rejected: key not authorized for this session".into(),
            ));
        }
    }
    // If no entry found for this pubkey, allow through (fallback compatibility).
}
```

- [x] **Step 4: Clear `allowed_entry_ids` in `with_client_pid`**

In `with_client_pid`, reset: `handler.allowed_entry_ids = None;`

- [x] **Step 5: Run tests**

Run: `cargo test --lib -p bw-agent`
Expected: ALL PASS

- [x] **Step 6: Commit**

```bash
git add crates/bw-agent/src/ssh_agent.rs
git commit -m "feat: integrate SSH key routing into request_identities and sign"
```

---

### Task 5: Commit the spec document

**Files:**
- Add: `docs/ssh-key-routing-spec.md` (already exists in worktree)

- [x] **Step 1: Commit spec**

```bash
git add docs/ssh-key-routing-spec.md
git commit -m "docs: add SSH key routing feature specification"
```

---

## Chunk 3: Final verification

### Task 6: Full test suite + clippy

- [x] **Step 1: Run all workspace tests**

Run: `cargo test --lib -p bw-core -p bw-agent`
Expected: ALL PASS

- [x] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: No warnings

- [x] **Step 3: Final commit if any fixes needed**

```bash
git add -A
git commit -m "chore: fix clippy warnings"
```
