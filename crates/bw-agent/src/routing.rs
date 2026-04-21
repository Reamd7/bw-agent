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
        if let Ok(glob) = globset::Glob::new(pattern) {
            if glob.compile_matcher().is_match(remote_url) {
                return true;
            }
        } else {
            log::warn!("Invalid glob pattern: {pattern}");
        }
    }
    false
}

/// Route entries based on remote URL and `gh-match` custom fields.
///
/// `pattern_extractor` is called for each entry to obtain its gh-match patterns.
/// This allows the caller to decrypt encrypted field names/values before matching.
///
/// Fallback cascade:
/// 1. matched + generic (routing hit)
/// 2. generic only (no hit, use generic keys)
/// 3. all entries (no generic keys, full compatibility)
pub fn route_entries<F>(
    entries: &[Entry],
    remote_url: Option<&str>,
    pattern_extractor: F,
) -> Vec<Entry>
where
    F: Fn(&Entry) -> Vec<String>,
{
    let Some(remote_url) = remote_url else {
        return entries.to_vec();
    };

    let mut matched = Vec::new();
    let mut generic = Vec::new();

    for entry in entries {
        let patterns = pattern_extractor(entry);
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
        assert!(matches_any_pattern(
            "github.com/mycompany/repo",
            &["github.com/mycompany/*".to_string()]
        ));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(matches_any_pattern(
            "github.com/mycompany/repo",
            &["github.com/mycompany/repo".to_string()]
        ));
    }

    #[test]
    fn test_glob_no_match() {
        assert!(!matches_any_pattern(
            "github.com/other/repo",
            &["github.com/mycompany/*".to_string()]
        ));
    }

    #[test]
    fn test_glob_match_multiple_stars() {
        assert!(matches_any_pattern(
            "github.com/mycompany-frontend/app",
            &["github.com/mycompany-*/*".to_string()]
        ));
    }

    #[test]
    fn test_glob_match_prefix_star() {
        assert!(matches_any_pattern(
            "github.com/any-org/any-repo",
            &["github.com/*/*".to_string()]
        ));
    }

    #[test]
    fn test_glob_match_double_star() {
        assert!(matches_any_pattern(
            "github.com/org/team/repo",
            &["github.com/**".to_string()]
        ));
    }

    #[test]
    fn test_glob_match_char_class() {
        assert!(matches_any_pattern(
            "github.com/org1/repo",
            &["github.com/org[0-9]/*".to_string()]
        ));
        assert!(!matches_any_pattern(
            "github.com/orgx/repo",
            &["github.com/org[0-9]/*".to_string()]
        ));
    }

    #[test]
    fn test_route_entries_single_match() {
        let work = make_ssh_entry("work", vec![make_field("gh-match", "github.com/company/*")]);
        let personal = make_ssh_entry("personal", vec![make_field("gh-match", "github.com/me/*")]);
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work, personal, generic.clone()];
        let result = route_entries(
            &entries,
            Some("github.com/company/repo"),
            extract_match_patterns,
        );

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|e| e.id == "work"));
        assert!(result.iter().any(|e| e.id == "generic"));
        assert!(!result.iter().any(|e| e.id == "personal"));
    }

    #[test]
    fn test_route_entries_fallback_generic() {
        let work = make_ssh_entry("work", vec![make_field("gh-match", "github.com/company/*")]);
        let personal = make_ssh_entry("personal", vec![make_field("gh-match", "github.com/me/*")]);
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work, personal, generic.clone()];
        let result = route_entries(
            &entries,
            Some("github.com/unknown/repo"),
            extract_match_patterns,
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "generic");
    }

    #[test]
    fn test_route_entries_fallback_all() {
        let work = make_ssh_entry("work", vec![make_field("gh-match", "github.com/company/*")]);
        let personal = make_ssh_entry("personal", vec![make_field("gh-match", "github.com/me/*")]);

        let entries = vec![work, personal];
        let result = route_entries(
            &entries,
            Some("github.com/unknown/repo"),
            extract_match_patterns,
        );

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_route_entries_no_remote_url() {
        let work = make_ssh_entry("work", vec![make_field("gh-match", "github.com/company/*")]);
        let entries = vec![work];
        let result = route_entries(&entries, None, extract_match_patterns);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_route_entries_excluded_not_in_generic_fallback() {
        let work = make_ssh_entry("work", vec![make_field("gh-match", "github.com/company/*")]);
        let generic = make_ssh_entry("generic", vec![]);

        let entries = vec![work, generic.clone()];
        let result = route_entries(
            &entries,
            Some("github.com/other/repo"),
            extract_match_patterns,
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "generic");
    }

    #[test]
    fn test_matches_any_pattern_empty_patterns() {
        assert!(!matches_any_pattern("github.com/org/repo", &[]));
    }
}
