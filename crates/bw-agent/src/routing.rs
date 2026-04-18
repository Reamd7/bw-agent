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
pub fn matches_any_pattern(remote_url: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if simple_glob_match(remote_url, pattern) {
            return true;
        }
    }
    false
}

/// Simple glob matching: supports `*` (any chars) and `?` (single char).
fn simple_glob_match(text: &str, pattern: &str) -> bool {
    let text_bytes = text.as_bytes();
    let pattern_bytes = pattern.as_bytes();

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
            && (pattern_bytes[pi] == text_bytes[ti] || pattern_bytes[pi] == b'?')
        {
            ti += 1;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern_bytes.len() && pattern_bytes[pi] == b'*' {
        pi += 1;
    }

    pi == pattern_bytes.len()
}

/// Route entries based on remote URL and `gh-match` custom fields.
///
/// Fallback cascade:
/// 1. matched + generic (routing hit)
/// 2. generic only (no hit, use generic keys)
/// 3. all entries (no generic keys, full compatibility)
pub fn route_entries(entries: &[Entry], remote_url: Option<&str>) -> Vec<Entry> {
    let Some(remote_url) = remote_url else {
        return entries.to_vec();
    };

    let mut matched = Vec::new();
    let mut generic = Vec::new();

    for entry in entries {
        let patterns = extract_match_patterns(entry);
        if patterns.is_empty() {
            generic.push(entry.clone());
        } else if matches_any_pattern(remote_url, &patterns) {
            matched.push(entry.clone());
        }
        // else: excluded, not added to any list
    }

    if !matched.is_empty() {
        let mut result = matched;
        result.extend(generic);
        result
    } else if !generic.is_empty() {
        generic
    } else {
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

        let entries = vec![work, personal, generic.clone()];
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

        let entries = vec![work, personal, generic.clone()];
        let result = route_entries(&entries, Some("github.com/unknown/repo"));

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
        assert_eq!(result.len(), 1);
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

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "generic");
    }

    #[test]
    fn test_matches_any_pattern_empty_patterns() {
        assert!(!matches_any_pattern("github.com/org/repo", &[]));
    }
}
