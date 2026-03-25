//! Pack content integrity verification.
//!
//! Checksums are computed over the canonical JSON representation of a pack
//! release's `files` map (sorted keys, no trailing whitespace). Both the
//! registry generator (`scripts/generate.py`) and this module use the same
//! canonical form so hashes match cross-platform.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::error::{Result, WeaveError};

/// Algorithm prefix for SHA-256 checksums in the registry.
const SHA256_PREFIX: &str = "sha256:";

/// Compute a `sha256:` prefixed checksum over a pack's files map.
///
/// The canonical form is `json.dumps(files, sort_keys=True, separators=(',', ':'))`,
/// i.e. compact JSON with keys sorted. This matches the Python registry generator.
pub fn compute(files: &HashMap<String, String>) -> String {
    let canonical = canonical_json(files);
    let hash = Sha256::digest(canonical.as_bytes());
    format!("{SHA256_PREFIX}{}", hex::encode(hash))
}

/// Verify a pack release's checksum. Returns `Ok(())` when:
/// - `expected` is `None` (old registry without checksums — warn and proceed)
/// - `expected` uses an unrecognized algorithm prefix (warn and proceed)
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
    // so future algorithm upgrades don't break old clients.
    if !expected.starts_with(SHA256_PREFIX) {
        log::warn!(
            "pack '{pack_name}' v{version} uses unknown checksum algorithm \
             '{}'; skipping verification",
            expected.split(':').next().unwrap_or("unknown")
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

    Ok(())
}

/// Produce compact sorted JSON for the files map — the canonical input to hash.
///
/// Equivalent to Python's `json.dumps(files, sort_keys=True, separators=(',', ':'))`.
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
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_known_hash() {
        let files = HashMap::from([("pack.toml".to_string(), "content".to_string())]);
        let checksum = compute(&files);
        assert!(checksum.starts_with("sha256:"));
        assert_eq!(checksum.len(), 7 + 64); // "sha256:" + 64 hex chars
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

    /// Cross-language validation: the canonical JSON for `{"a":"1","b":"2"}`
    /// must produce the same SHA-256 as Python's
    /// `hashlib.sha256(json.dumps({"a":"1","b":"2"}, sort_keys=True, separators=(",",":")).encode()).hexdigest()`
    #[test]
    fn cross_language_checksum_matches_python() {
        // Python: json.dumps({"a":"1","b":"2"}, sort_keys=True, separators=(",",":"))
        //       = '{"a":"1","b":"2"}'
        // hashlib.sha256(b'{"a":"1","b":"2"}').hexdigest()
        //       = "21f76dfbfe6dfe21f762080ef484112cf2952974cef30741fd1931e1c6d92112"
        let files = HashMap::from([
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "1".to_string()),
        ]);
        assert_eq!(
            compute(&files),
            "sha256:21f76dfbfe6dfe21f762080ef484112cf2952974cef30741fd1931e1c6d92112"
        );
    }
}
