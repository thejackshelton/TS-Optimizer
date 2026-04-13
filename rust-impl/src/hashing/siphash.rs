/// SipHash-1-3 hashing for Qwik symbol names.
///
/// Replicates Rust's DefaultHasher (SipHash-1-3 with zero keys) and
/// Qwik's base64 encoding (URL-safe, no padding, replace - and _ with 0).
use std::hash::Hasher;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use siphasher::sip::SipHasher13;

/// Compute a Qwik-compatible hash for a symbol.
///
/// Hash input is raw concatenated bytes: scope + rel_path + display_name (no separators).
/// Uses SipHash-1-3 with keys (0,0).
/// Returns 11-character base64url-encoded hash string with - and _ replaced by 0.
pub fn qwik_hash(scope: Option<&str>, rel_path: &str, display_name: &str) -> String {
    // HASH-02: Hash input is raw concatenated bytes
    let mut hasher = SipHasher13::new_with_keys(0, 0); // siphasher crate
    if let Some(s) = scope {
        hasher.write(s.as_bytes());
    }
    hasher.write(rel_path.as_bytes());
    hasher.write(display_name.as_bytes());

    // HASH-01: Get the u64 result
    let hash = hasher.finish();

    // HASH-03: u64 little-endian bytes → base64url, replace - and _ with 0
    let bytes = hash.to_le_bytes();
    let encoded = URL_SAFE_NO_PAD.encode(bytes);

    // Replace - and _ with 0
    encoded.replace(['-', '_'], "0")
}

/// Compute the file hash for JSX key prefix generation.
/// This matches SWC's file_hash: SipHash13(scope + rel_path) only.
/// Returns the full base64url string (first 2 chars used as key prefix).
pub fn file_hash_base64(scope: Option<&str>, rel_path: &str) -> String {
    let mut hasher = SipHasher13::new_with_keys(0, 0);
    if let Some(s) = scope {
        hasher.write(s.as_bytes());
    }
    hasher.write(rel_path.as_bytes());

    let hash = hasher.finish();
    let bytes = hash.to_le_bytes();
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    encoded.replace(['-', '_'], "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_hash() {
        // From snapshot: test.tsx, renderHeader1 → jMxQsjbyDss
        let hash = qwik_hash(None, "test.tsx", "renderHeader1");
        assert_eq!(hash, "jMxQsjbyDss");
    }

    #[test]
    fn test_known_hash_nested() {
        // From snapshot: test.tsx, renderHeader1_div_onClick → USi8k1jUb40
        let hash = qwik_hash(None, "test.tsx", "renderHeader1_div_onClick");
        assert_eq!(hash, "USi8k1jUb40");
    }

    #[test]
    fn test_known_hash_component() {
        // From snapshot: test.tsx, SecretForm_component → 1noi8FsTz7c
        let hash = qwik_hash(None, "test.tsx", "SecretForm_component");
        assert_eq!(hash, "1noi8FsTz7c");
    }

    #[test]
    fn test_known_hash_global_action() {
        // From snapshot: test.tsx, useSecretAction_globalAction → Cbn41AEUQ0Q
        let hash = qwik_hash(None, "test.tsx", "useSecretAction_globalAction");
        assert_eq!(hash, "Cbn41AEUQ0Q");
    }

    #[test]
    fn test_file_hash_key_prefix() {
        // Key prefix for test.tsx should be "u6" (from snapshots)
        let hash = file_hash_base64(None, "test.tsx");
        assert_eq!(&hash[..2], "u6", "Key prefix for test.tsx should be 'u6', got '{}'", &hash[..2]);
    }

    #[test]
    fn test_known_hash_renderheader2() {
        // From snapshot: test.tsx, renderHeader2_component → Ay6ibkfFYsw
        let hash = qwik_hash(None, "test.tsx", "renderHeader2_component");
        assert_eq!(hash, "Ay6ibkfFYsw");
    }

}
