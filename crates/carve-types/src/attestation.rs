//! BLAKE3-sealed backup tag — cordel-style attestation.
//!
//! Every time carve mutates branch state, it first creates a backup tag
//! whose annotated-tag message embeds a BLAKE3 hash over the canonical
//! plan + the original branch tip. After execution, the operator can
//! verify recovery integrity by re-hashing and comparing.
//!
//! The hash is **not** a security primitive (anyone with push access can
//! create a tag); it's a **drift detector**. If the recovery tag's stored
//! hash diverges from what re-computation yields, *something else mutated
//! the recovery anchor* and the operator should investigate before relying
//! on it.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// 64-hex-char BLAKE3 digest, validated on construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlakeHash(String);

impl BlakeHash {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidBlakeHash { hash: s });
        }
        Ok(Self(s))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = blake3::hash(bytes);
        Self(digest.to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BlakeHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Metadata embedded in a carve backup tag.
///
/// Carve writes this as YAML inside the annotated-tag message. The tag
/// **name** is `carve-backup/<TICKET>-pre-carve-<YYYYMMDD-HHmmss>`; the
/// **target** is the original branch tip; the **message** body is the
/// YAML of this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupTag {
    pub tag_name: String,
    pub original_tip: String,
    pub original_branch: String,
    pub plan_hash: BlakeHash,
    pub created_at: String,
    /// Carve binary version that wrote the tag — useful when reading old
    /// backups whose format may have evolved.
    pub carve_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake_hash_validates() {
        assert!(BlakeHash::new("a".repeat(64)).is_ok());
        assert!(BlakeHash::new("a".repeat(63)).is_err()); // too short
        assert!(BlakeHash::new("g".repeat(64)).is_err()); // non-hex
    }

    #[test]
    fn blake_hash_from_bytes_is_64_hex_chars() {
        let h = BlakeHash::from_bytes(b"hello world");
        assert_eq!(h.as_str().len(), 64);
        assert!(h.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }
}
