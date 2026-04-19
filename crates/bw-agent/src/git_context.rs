//! Git repository context extraction for SSH key routing.

use crate::process::ProcessInfo;
use std::path::Path;

/// Normalize a raw git remote URL to `{host}/{owner}/{repo}` format.
///
/// Returns None for non-network URLs (local paths, etc.).
pub fn normalize_remote_url(raw_url: &str) -> Option<String> {
    let url = raw_url.trim();

    let url = url.strip_suffix(".git").unwrap_or(url);
    let url = url.trim_end_matches('/');

    // Detect URI-style schemes (ssh://, git://, https://, http://).
    // These use host[:port]/path syntax, NOT SCP host:path syntax.
    let (url, is_uri_scheme) = if let Some(stripped) = url.strip_prefix("ssh://") {
        (stripped, true)
    } else if let Some(stripped) = url.strip_prefix("git://") {
        (stripped, true)
    } else if let Some(stripped) = url.strip_prefix("https://") {
        (stripped, true)
    } else if let Some(stripped) = url.strip_prefix("http://") {
        (stripped, true)
    } else {
        (url, false)
    };

    // Strip username (e.g. "git@").
    let url = if let Some(at_pos) = url.find('@') {
        &url[at_pos + 1..]
    } else {
        url
    };

    // For URI-style URLs, we already have host[:port]/path — the colon
    // (if any) is a port separator, NOT a path separator. Just use as-is.
    // For SCP-style URLs, convert host:path to host/path.
    let url = if !is_uri_scheme {
        if let Some(colon_pos) = url.find(':') {
            let before_colon = &url[..colon_pos];
            let after_colon = &url[colon_pos + 1..];
            if !before_colon.contains('/') && !after_colon.starts_with('/') {
                format!("{}/{}", before_colon, after_colon)
            } else {
                url.to_string()
            }
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };

    let segments: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 3 {
        return None;
    }
    // First segment must look like a hostname (contains '.' or is 'localhost').
    if !segments[0].contains('.') && segments[0] != "localhost" {
        return None;
    }

    Some(url.to_ascii_lowercase())
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
        if (exe_lower == "git" || exe_lower == "git.exe")
            && !proc_info.cwd.is_empty()
            && proc_info.cwd != "unknown"
        {
            return Some(&proc_info.cwd);
        }
    }
    None
}

/// Locate the git config file from a working directory.
fn find_git_config(start_dir: &Path) -> Option<std::path::PathBuf> {
    let mut dir = start_dir.to_path_buf();

    for _ in 0..10 {
        let git_path = dir.join(".git");

        if git_path.is_dir() {
            let config = git_path.join("config");
            if config.exists() {
                return Some(config);
            }
        } else if git_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&git_path) {
                if let Some(gitdir) = content.strip_prefix("gitdir: ") {
                    let gitdir = gitdir.trim();
                    let gitdir_path = if std::path::Path::new(gitdir).is_absolute() {
                        std::path::PathBuf::from(gitdir)
                    } else {
                        dir.join(gitdir)
                    };

                    // Try gitdir/config directly (submodules, some worktrees).
                    let config = gitdir_path.join("config");
                    if config.exists() {
                        return Some(config);
                    }

                    // Worktree: gitdir is an admin dir without config.
                    // Read commondir to find the shared repository.
                    let commondir_path = gitdir_path.join("commondir");
                    if let Ok(common) = std::fs::read_to_string(&commondir_path) {
                        let common = common.trim();
                        let common_path = if std::path::Path::new(common).is_absolute() {
                            std::path::PathBuf::from(common)
                        } else {
                            gitdir_path.join(common)
                        };
                        let config = common_path.join("config");
                        if config.exists() {
                            return Some(config);
                        }
                    }
                }
            }
        } else if dir.join("HEAD").exists() && dir.join("config").exists() {
            return Some(dir.join("config"));
        }

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

/// Read the current branch name from a git config directory.
///
/// Returns None if HEAD is detached or cannot be read.
fn read_current_branch(config_dir: &Path) -> Option<String> {
    let head_path = config_dir.join("HEAD");
    let head = std::fs::read_to_string(&head_path).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/").map(|s| s.to_string())
}

/// Determine which remote to use for the current branch.
///
/// Order: branch upstream remote → "origin" → None (ambiguous).
fn resolve_remote_name(config_path: &Path, config_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(config_path).ok()?;

    // Try current branch upstream first.
    if let Some(branch) = read_current_branch(config_dir) {
        let branch_remote_header = format!("[branch \"{}\"]", branch);
        let mut in_section = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_section = trimmed.starts_with(&branch_remote_header);
                continue;
            }
            if in_section {
                if let Some(remote) = trimmed.strip_prefix("remote = ") {
                    let remote = remote.trim().to_string();
                    if !remote.is_empty() {
                        return Some(remote);
                    }
                }
                if let Some(remote) = trimmed.strip_prefix("remote=") {
                    let remote = remote.trim().to_string();
                    if !remote.is_empty() {
                        return Some(remote);
                    }
                }
            }
        }
    }

    // Fallback to "origin" if it exists.
    if extract_remote_url_from_config(config_path, "origin").is_some() {
        return Some("origin".to_string());
    }

    // Check for ambiguity: if multiple remotes exist, return None.
    let mut found_remotes = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[remote \"") {
            if let Some(end) = rest.find("\"]") {
                found_remotes.push(rest[..end].to_string());
            }
        }
    }
    if found_remotes.len() == 1 {
        return Some(found_remotes.pop().unwrap());
    }

    None
}

/// Extract a normalized remote URL from the process chain.
pub fn extract_remote_url(process_chain: &[ProcessInfo]) -> Option<String> {
    let cwd = find_git_cwd(process_chain)?;
    let config_path = find_git_config(Path::new(cwd))?;

    // Determine the config directory for reading HEAD.
    let config_dir = config_path.parent().unwrap_or(Path::new("."));

    let remote_name = resolve_remote_name(&config_path, config_dir)?;
    let raw_url = extract_remote_url_from_config(&config_path, &remote_name)?;
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
    fn test_normalize_url_case_insensitive() {
        assert_eq!(
            normalize_remote_url("git@GitHub.com:Reamd7/bw-agent.git"),
            Some("github.com/reamd7/bw-agent".to_string())
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
        assert_eq!(
            result,
            Some("git@github.com:mycompany/repo.git".to_string())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- New tests for review fixes ---

    #[test]
    fn test_normalize_url_ssh_with_port() {
        assert_eq!(
            normalize_remote_url("ssh://git@github.com:2222/mycompany/repo.git"),
            Some("github.com:2222/mycompany/repo".to_string())
        );
    }

    #[test]
    fn test_normalize_url_https_with_port() {
        assert_eq!(
            normalize_remote_url("https://github.com:443/mycompany/repo.git"),
            Some("github.com:443/mycompany/repo".to_string())
        );
    }

    #[test]
    fn test_find_git_config_worktree_via_commondir() {
        // Simulate a git worktree layout:
        //   worktree/.git (file) → gitdir: <admin_dir>
        //   <admin_dir>/commondir → ../../ (points to main repo)
        //   <main_repo>/.git/config (the actual config)
        let tmp = std::env::temp_dir().join("bw-agent-test-worktree");
        let _ = std::fs::remove_dir_all(&tmp);

        // Main repo: tmp/main/.git/config
        let main_git = tmp.join("main").join(".git");
        std::fs::create_dir_all(&main_git).unwrap();
        std::fs::write(
            main_git.join("config"),
            "[remote \"origin\"]\n    url = git@github.com:org/repo.git\n",
        )
        .unwrap();

        // Worktree admin dir: tmp/main/.git/worktrees/wt
        let admin_dir = main_git.join("worktrees").join("wt");
        std::fs::create_dir_all(&admin_dir).unwrap();
        // commondir points to main repo's .git dir (relative from admin dir)
        std::fs::write(admin_dir.join("commondir"), "../../../.git").unwrap();

        // Worktree working dir: tmp/worktree/.git (file pointing to admin dir)
        let worktree_dir = tmp.join("worktree");
        std::fs::create_dir_all(&worktree_dir).unwrap();
        // Use forward slashes for cross-platform compat in the gitdir file
        let gitdir_content = format!("gitdir: {}", admin_dir.to_string_lossy().replace('\\', "/"));
        std::fs::write(worktree_dir.join(".git"), gitdir_content).unwrap();

        // find_git_config should follow .git → admin_dir → commondir → main config
        let result = find_git_config(&worktree_dir);
        assert!(
            result.is_some(),
            "find_git_config should resolve worktree via commondir"
        );
        let config_path = result.unwrap();
        assert!(
            config_path.ends_with("config"),
            "should point to the main repo config"
        );

        let url = extract_remote_url_from_config(&config_path, "origin");
        assert_eq!(url, Some("git@github.com:org/repo.git".to_string()));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_resolve_remote_branch_upstream() {
        let dir = std::env::temp_dir().join("bw-agent-test-branch-remote");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[remote "origin"]
    url = git@github.com:org/public.git
[remote "upstream"]
    url = git@github.com:org/private.git
[branch "feature"]
    remote = upstream
"#;
        std::fs::write(dir.join("config"), config_content).unwrap();
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/feature\n").unwrap();

        let remote = resolve_remote_name(&dir.join("config"), &dir);
        assert_eq!(remote, Some("upstream".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_remote_fallback_to_origin() {
        let dir = std::env::temp_dir().join("bw-agent-test-origin-fallback");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[remote "origin"]
    url = git@github.com:org/repo.git
[branch "main"]
"#;
        std::fs::write(dir.join("config"), config_content).unwrap();
        // HEAD points to a branch with no remote configured → fallback to origin
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let remote = resolve_remote_name(&dir.join("config"), &dir);
        assert_eq!(remote, Some("origin".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_remote_ambiguous_no_origin() {
        let dir = std::env::temp_dir().join("bw-agent-test-ambiguous");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[remote "upstream"]
    url = git@github.com:org/repo.git
[remote "mirror"]
    url = git@gitlab.com:org/repo.git
"#;
        std::fs::write(dir.join("config"), config_content).unwrap();
        // Detached HEAD, no branch remote, no "origin"
        std::fs::write(dir.join("HEAD"), "abc123\n").unwrap();

        // No origin, multiple remotes → ambiguous
        let remote = resolve_remote_name(&dir.join("config"), &dir);
        assert_eq!(
            remote, None,
            "multiple remotes with no origin and no branch should be ambiguous"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_remote_prefers_origin_over_ambiguous() {
        let dir = std::env::temp_dir().join("bw-agent-test-origin-preferred");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[remote "origin"]
    url = git@github.com:org/repo.git
[remote "mirror"]
    url = git@gitlab.com:org/repo.git
"#;
        std::fs::write(dir.join("config"), config_content).unwrap();
        std::fs::write(dir.join("HEAD"), "abc123\n").unwrap();

        // Even with multiple remotes, origin exists → prefer it
        let remote = resolve_remote_name(&dir.join("config"), &dir);
        assert_eq!(remote, Some("origin".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_remote_single_non_origin() {
        let dir = std::env::temp_dir().join("bw-agent-test-single-remote");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[remote "myremote"]
    url = git@github.com:org/repo.git
"#;
        std::fs::write(dir.join("config"), config_content).unwrap();
        std::fs::write(dir.join("HEAD"), "abc123\n").unwrap();

        // Single non-origin remote → should resolve to it
        let remote = resolve_remote_name(&dir.join("config"), &dir);
        assert_eq!(remote, Some("myremote".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_current_branch() {
        let dir = std::env::temp_dir().join("bw-agent-test-head");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Normal branch
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(read_current_branch(&dir), Some("main".to_string()));

        // Detached HEAD
        std::fs::write(dir.join("HEAD"), "abc123def456\n").unwrap();
        assert_eq!(read_current_branch(&dir), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_find_git_config_subdirectory() {
        let tmp = std::env::temp_dir().join("bw-agent-test-subdir");
        let _ = std::fs::remove_dir_all(&tmp);

        // Create repo at tmp/repo/.git/config
        let git_dir = tmp.join("repo").join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(
            git_dir.join("config"),
            "[remote \"origin\"]\n    url = git@github.com:org/repo.git\n",
        )
        .unwrap();

        // Create subdirectory: tmp/repo/src/components
        let subdir = tmp.join("repo").join("src").join("components");
        std::fs::create_dir_all(&subdir).unwrap();

        // find_git_config from subdir should walk up and find the config
        let result = find_git_config(&subdir);
        assert!(
            result.is_some(),
            "should find config by walking up from subdirectory"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
