use hmac::Mac;
use sha2::Sha256;

/// Verifies a webhook signature (constant-time).
pub fn verify_webhook_signature(secret: &str, raw_body: &[u8], header_value: &str) -> bool {
    let Some(expected) = header_value.strip_prefix("sha256=") else {
        return false;
    };
    let mut mac = hmac::Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(raw_body);
    let digest = hex::encode(&mac.finalize().into_bytes());
    constant_time_eq(digest.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Returns the changed-file set from a push payload, or None when the push
/// should be treated as "all files changed" (forced push, >20 commits).
pub fn changed_files_from_push(payload: &serde_json::Value) -> Option<Vec<String>> {
    let commits = payload.get("commits")?.as_array()?;
    if commits.len() > 20 {
        return None;
    }
    let mut files: Vec<String> = Vec::new();
    for commit in commits {
        for key in ["added", "modified", "removed"] {
            if let Some(list) = commit[key].as_array() {
                for file in list {
                    if let Some(name) = file.as_str() {
                        if !files.iter().any(|f| f == name) {
                            files.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    Some(files)
}

/// Glob-ish path filter: supports `dir/`, `dir/**`, `*.ext`, and exact
/// paths. Empty pattern matches everything.
pub fn path_matches(pattern: &str, changed: &[String]) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return true;
    }
    changed.iter().any(|file| {
        if pattern.ends_with("/**") {
            let prefix = pattern.trim_end_matches('*').trim_end_matches('/');
            file == prefix || file.starts_with(&format!("{prefix}/"))
        } else if pattern.ends_with('/') {
            file.starts_with(pattern)
        } else if pattern.contains('*') {
            let prefix = pattern.split('*').next().unwrap_or_default();
            file.starts_with(prefix)
        } else {
            file == pattern || file.starts_with(&format!("{pattern}/"))
        }
    })
}

/// Exposes hex for the HMAC digest without adding the hex crate at every
/// call site.
pub mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::Hmac;

    #[test]
    fn webhook_signature_validates_hmac() {
        let secret = "whsec_test";
        let body = br#"{"action":"push"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let good = format!("sha256={}", hex::encode(&mac.finalize().into_bytes()));
        assert!(verify_webhook_signature(secret, body, &good));
        assert!(!verify_webhook_signature(secret, body, "sha256=deadbeef"));
        assert!(!verify_webhook_signature(secret, body, "no-prefix"));
        assert!(!verify_webhook_signature("other", body, &good));
    }

    #[test]
    fn changed_files_extract_and_cap() {
        let payload = serde_json::json!({
            "commits": [
                {"added": ["a/x.rs"], "modified": ["b/y.rs"], "removed": []},
                {"added": [], "modified": ["b/z.rs"], "removed": ["old"]},
            ]
        });
        let files = changed_files_from_push(&payload).unwrap();
        assert_eq!(files.len(), 4);
        let big = serde_json::json!({
            "commits": (0..21).map(|i| serde_json::json!({"modified": [format!("f{i}")]})).collect::<Vec<_>>()
        });
        assert!(changed_files_from_push(&big).is_none());
    }

    #[test]
    fn path_globs_match_monorepo_layouts() {
        let changed = vec![
            "services/api/src/main.rs".to_owned(),
            "services/web/index.html".to_owned(),
            "README.md".to_owned(),
        ];
        assert!(path_matches("", &changed));
        assert!(path_matches("services/api", &changed));
        assert!(path_matches("services/api/**", &changed));
        assert!(path_matches("services/", &changed));
        assert!(path_matches("services/web", &changed)); // web/index.html changed
        assert!(!path_matches("packages/ui", &changed));
        assert!(!path_matches(
            "services/web",
            &["services/api/src/main.rs".to_owned()]
        ));
    }
}
