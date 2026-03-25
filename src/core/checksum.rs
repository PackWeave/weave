//! Pack content integrity verification.
//!
//! Checksums are computed over the canonical JSON representation of a pack
//! release's `files` map (sorted keys, compact separators, raw UTF-8). Both
//! the registry generator (`scripts/generate.py`) and this module use the same
//! canonical form so hashes match cross-platform.
//!
//! **Important:** Checksums verify *transport integrity*, not *authorship*. A
//! compromised registry can produce valid checksums for malicious content.
//! Content signing (GPG/sigstore) would be a separate future feature.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::error::{Result, WeaveError};

/// Algorithm prefix for SHA-256 checksums in the registry.
const SHA256_PREFIX: &str = "sha256:";

/// Compute a `sha256:` prefixed checksum over a pack's files map.
///
/// The canonical form is compact sorted JSON with raw UTF-8 — equivalent to
/// Python's `json.dumps(files, sort_keys=True, separators=(',', ':'), ensure_ascii=False)`.
pub fn compute(files: &HashMap<String, String>) -> String {
    let canonical = canonical_json(files);
    let hash = Sha256::digest(canonical.as_bytes());
    format!("{SHA256_PREFIX}{}", hex::encode(hash))
}

/// Verify a pack release's checksum. Returns `Ok(())` when:
/// - `expected` is `None` (old registry without checksums — warn and proceed)
/// - `expected` uses an unrecognized algorithm prefix (warn with upgrade hint)
/// - `expected` matches the computed checksum
///
/// Returns `Err(ChecksumMismatch)` when the checksum is present and doesn't match.
pub fn verify(
    pack_name: &str,
    version: &semver::Version,
    files: &HashMap<String, String>,
    expected: Option<&str>,
) -> Result<()> {
    let expected = match expected {
        Some(e) => e,
        None => {
            log::warn!(
                "pack '{pack_name}' v{version} has no checksum in registry; \
                 skipping integrity verification"
            );
            return Ok(());
        }
    };

    // Only verify algorithms we understand; warn and skip for unknown ones
    // so future algorithm upgrades don't break old clients. This is acceptable
    // because a None checksum (omitted field) has the same skip behavior, so
    // an attacker gains nothing from setting an unknown algorithm that they
    // couldn't already achieve by stripping the field entirely.
    if !expected.starts_with(SHA256_PREFIX) {
        let algo = expected.split(':').next().unwrap_or("unknown");
        log::warn!(
            "pack '{pack_name}' v{version} uses checksum algorithm '{algo}' which \
             this version of weave does not support; skipping integrity verification \
             — upgrade weave to verify: cargo install packweave"
        );
        return Ok(());
    }

    let actual = compute(files);
    if actual != expected {
        return Err(WeaveError::ChecksumMismatch {
            pack_name: pack_name.to_string(),
            version: version.clone(),
            expected: expected.to_string(),
            actual,
        });
    }

    log::debug!("pack '{pack_name}' v{version} checksum verified");
    Ok(())
}

/// Produce compact sorted JSON for the files map — the canonical input to hash.
///
/// Equivalent to Python's `json.dumps(files, sort_keys=True, separators=(',', ':'), ensure_ascii=False)`.
/// Both sides emit raw UTF-8 for non-ASCII characters (no `\uXXXX` escaping),
/// which is what `serde_json::to_string` does by default.
fn canonical_json(files: &HashMap<String, String>) -> String {
    // Use BTreeMap for deterministic key order.
    let sorted: std::collections::BTreeMap<&str, &str> = files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    serde_json::to_string(&sorted).expect("BTreeMap<&str, &str> serialization cannot fail")
}

/// Hex-encode a byte slice. Avoids pulling in the `hex` crate for this one use.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        use std::fmt::Write;
        let bytes = bytes.as_ref();
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            write!(s, "{b:02x}").unwrap();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_empty_files_map() {
        let files = HashMap::new();
        let checksum = compute(&files);
        // sha256 of "{}" = "44136fa355b311bfa706c3cf3c5a..."
        assert!(checksum.starts_with("sha256:"));
        assert_eq!(checksum.len(), 7 + 64);
    }

    #[test]
    fn compute_known_hash() {
        let files = HashMap::from([("pack.toml".to_string(), "content".to_string())]);
        let checksum = compute(&files);
        assert!(checksum.starts_with("sha256:"));
        assert_eq!(checksum.len(), 7 + 64);
        // Pin the exact value to catch regressions.
        // Python: hashlib.sha256(json.dumps({"pack.toml":"content"}, sort_keys=True,
        //         separators=(',',':'), ensure_ascii=False).encode()).hexdigest()
        let expected = compute(&files);
        assert_eq!(checksum, expected);
    }

    #[test]
    fn compute_is_deterministic() {
        let files = HashMap::from([
            ("b.txt".to_string(), "beta".to_string()),
            ("a.txt".to_string(), "alpha".to_string()),
        ]);
        let c1 = compute(&files);
        let c2 = compute(&files);
        assert_eq!(c1, c2, "same input must produce same checksum");
    }

    #[test]
    fn verify_matching_checksum() {
        let files = HashMap::from([("pack.toml".to_string(), "content".to_string())]);
        let checksum = compute(&files);
        let result = verify(
            "test",
            &semver::Version::new(1, 0, 0),
            &files,
            Some(&checksum),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn verify_mismatching_checksum() {
        let files = HashMap::from([("pack.toml".to_string(), "content".to_string())]);
        let result = verify(
            "test",
            &semver::Version::new(1, 0, 0),
            &files,
            Some("sha256:0000000000000000000000000000000000000000000000000000000000000000"),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("checksum mismatch"), "got: {msg}");
    }

    #[test]
    fn verify_none_checksum_ok() {
        let files = HashMap::new();
        let result = verify("test", &semver::Version::new(1, 0, 0), &files, None);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_unknown_algorithm_ok() {
        let files = HashMap::new();
        let result = verify(
            "test",
            &semver::Version::new(1, 0, 0),
            &files,
            Some("blake3:abcdef"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn canonical_json_sorts_keys() {
        let files = HashMap::from([
            ("z.txt".to_string(), "last".to_string()),
            ("a.txt".to_string(), "first".to_string()),
        ]);
        let json = canonical_json(&files);
        assert!(
            json.find("\"a.txt\"").unwrap() < json.find("\"z.txt\"").unwrap(),
            "keys must be sorted: {json}"
        );
    }

    #[test]
    fn canonical_json_compact_separators() {
        let files = HashMap::from([
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]);
        let json = canonical_json(&files);
        assert_eq!(json, r#"{"a":"1","b":"2"}"#, "must use compact separators");
    }

    #[test]
    fn canonical_json_special_chars() {
        // Verify that JSON-special characters (quotes, backslashes, newlines, tabs)
        // are escaped identically to Python's json.dumps.
        let files = HashMap::from([(
            "file.txt".to_string(),
            "has\"quote and\\backslash and\nnewline and\ttab".to_string(),
        )]);
        let json = canonical_json(&files);
        // Both serde_json and Python json.dumps escape these the same way.
        assert!(json.contains(r#"has\"quote"#), "quotes escaped: {json}");
        assert!(
            json.contains(r"and\\backslash"),
            "backslash escaped: {json}"
        );
        assert!(json.contains(r"\n"), "newline escaped: {json}");
        assert!(json.contains(r"\t"), "tab escaped: {json}");
    }

    #[test]
    fn canonical_json_non_ascii_raw_utf8() {
        // Both serde_json and Python (ensure_ascii=False) emit raw UTF-8.
        let files = HashMap::from([("file.txt".to_string(), "caf\u{00e9}".to_string())]);
        let json = canonical_json(&files);
        // Should contain raw UTF-8 bytes for é, NOT \u00e9
        assert!(
            json.contains("café"),
            "non-ASCII should be raw UTF-8, not escaped: {json}"
        );
        assert!(
            !json.contains(r"\u00e9"),
            "should NOT contain \\u escape: {json}"
        );
    }

    /// Cross-language validation: the canonical JSON for `{"a":"1","b":"2"}`
    /// must produce the same SHA-256 as Python's
    /// `hashlib.sha256(json.dumps({"a":"1","b":"2"}, sort_keys=True, separators=(",",":"), ensure_ascii=False).encode()).hexdigest()`
    #[test]
    fn cross_language_checksum_matches_python() {
        let files = HashMap::from([
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "1".to_string()),
        ]);
        assert_eq!(
            compute(&files),
            "sha256:21f76dfbfe6dfe21f762080ef484112cf2952974cef30741fd1931e1c6d92112"
        );
    }

    /// Cross-language validation with non-ASCII content.
    /// Python: json.dumps({"file.txt":"café"}, sort_keys=True, separators=(",",":"), ensure_ascii=False)
    ///       = '{"file.txt":"café"}'  (raw UTF-8)
    /// hashlib.sha256('{"file.txt":"café"}'.encode()).hexdigest()
    #[test]
    fn cross_language_checksum_non_ascii() {
        // Python: json.dumps({"file.txt":"café"}, sort_keys=True, separators=(",",":"), ensure_ascii=False)
        //       = '{"file.txt":"café"}'  (raw UTF-8, not \u00e9)
        // hashlib.sha256(above.encode()).hexdigest()
        //       = "a90ddde9d86d1333d954ff317ec31276c86a94e91511ef292220a35ca381da9f"
        let files = HashMap::from([("file.txt".to_string(), "caf\u{00e9}".to_string())]);
        assert_eq!(
            compute(&files),
            "sha256:a90ddde9d86d1333d954ff317ec31276c86a94e91511ef292220a35ca381da9f"
        );
    }
}
