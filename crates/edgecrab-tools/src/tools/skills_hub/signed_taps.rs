//! Signed publisher taps — Ed25519 + content sha256 + TOFU pins (019 W4).
//!
//! Unsigned taps remain community trust. Signed manifests never silently elevate
//! on verification failure (fail closed).

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::hub_dir;

pub const SIGNED_TAP_SCHEMA: &str = "edgecrab.signed-tap/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTapManifest {
    pub schema: String,
    pub publisher: String,
    pub public_key_b64: String,
    pub skills: Vec<SignedSkillEntry>,
    pub signature_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedSkillEntry {
    pub name: String,
    pub content_sha256: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedSignedTap {
    pub publisher: String,
    pub public_key_b64: String,
    pub key_id: String,
    pub skills: BTreeMap<String, String>, // name -> sha256 hex
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PublisherPins {
    /// publisher → public_key_b64
    #[serde(default)]
    pins: BTreeMap<String, String>,
}

fn pins_path() -> std::path::PathBuf {
    hub_dir().join("publisher_pins.json")
}

fn read_pins() -> PublisherPins {
    let path = pins_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PublisherPins::default(),
    }
}

fn write_pins(pins: &PublisherPins) -> Result<(), String> {
    let path = pins_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(pins).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Short key id (first 16 hex chars of sha256(pubkey bytes)).
pub fn key_id_from_public_key_b64(public_key_b64: &str) -> Result<String, String> {
    let bytes = B64
        .decode(public_key_b64.trim())
        .map_err(|e| format!("invalid public_key_b64: {e}"))?;
    let digest = Sha256::digest(&bytes);
    Ok(hex::encode(&digest[..8]))
}

/// Canonical bytes signed by the publisher (excludes signature field).
pub fn canonical_manifest_message(manifest: &SignedTapManifest) -> Vec<u8> {
    let mut lines = vec![
        format!("{SIGNED_TAP_SCHEMA}"),
        format!("publisher={}", manifest.publisher.trim()),
        format!("public_key={}", manifest.public_key_b64.trim()),
    ];
    let mut skills = manifest.skills.clone();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    for skill in skills {
        lines.push(format!(
            "skill={}:{}",
            skill.name.trim(),
            skill.content_sha256.trim().to_ascii_lowercase()
        ));
    }
    lines.join("\n").into_bytes()
}

pub fn content_sha256_hex(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content).as_slice())
}

/// Sign a manifest with a SigningKey (tests / publisher tooling).
pub fn sign_manifest(
    mut manifest: SignedTapManifest,
    signing_key: &SigningKey,
) -> Result<SignedTapManifest, String> {
    manifest.schema = SIGNED_TAP_SCHEMA.into();
    let vk = signing_key.verifying_key();
    manifest.public_key_b64 = B64.encode(vk.as_bytes());
    let msg = canonical_manifest_message(&manifest);
    let sig = signing_key.sign(&msg);
    manifest.signature_b64 = B64.encode(sig.to_bytes());
    Ok(manifest)
}

/// Verify signature + schema. Does not check TOFU pins.
pub fn verify_signed_manifest(manifest: &SignedTapManifest) -> Result<VerifiedSignedTap, String> {
    if manifest.schema != SIGNED_TAP_SCHEMA {
        return Err(format!(
            "unsupported signed-tap schema '{}'",
            manifest.schema
        ));
    }
    if manifest.publisher.trim().is_empty() {
        return Err("signed-tap publisher is empty".into());
    }
    let pk_bytes = B64
        .decode(manifest.public_key_b64.trim())
        .map_err(|e| format!("invalid public_key_b64: {e}"))?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "public_key_b64 must decode to 32 bytes".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&pk_arr).map_err(|e| format!("invalid public key: {e}"))?;

    let sig_bytes = B64
        .decode(manifest.signature_b64.trim())
        .map_err(|e| format!("invalid signature_b64: {e}"))?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "signature_b64 must decode to 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&sig_arr);

    let msg = canonical_manifest_message(manifest);
    verifying_key
        .verify(&msg, &signature)
        .map_err(|_| "signed-tap signature verification failed (fail closed)".to_string())?;

    let mut skills = BTreeMap::new();
    for entry in &manifest.skills {
        if entry.name.trim().is_empty() || entry.content_sha256.trim().is_empty() {
            return Err("signed-tap skill entry missing name or content_sha256".into());
        }
        skills.insert(
            entry.name.trim().to_string(),
            entry.content_sha256.trim().to_ascii_lowercase(),
        );
    }

    Ok(VerifiedSignedTap {
        publisher: manifest.publisher.trim().to_string(),
        public_key_b64: manifest.public_key_b64.trim().to_string(),
        key_id: key_id_from_public_key_b64(&manifest.public_key_b64)?,
        skills,
    })
}

/// Assert skill file content matches the verified manifest hash (fail closed).
pub fn assert_content_hash(
    verified: &VerifiedSignedTap,
    skill_name: &str,
    content: &[u8],
) -> Result<(), String> {
    let Some(expected) = verified.skills.get(skill_name) else {
        return Err(format!(
            "skill '{skill_name}' not listed in signed manifest for publisher '{}'",
            verified.publisher
        ));
    };
    let actual = content_sha256_hex(content);
    if &actual != expected {
        return Err(format!(
            "content hash mismatch for '{skill_name}': expected {expected}, got {actual} (fail closed)"
        ));
    }
    Ok(())
}

/// TOFU pin publisher key. Rotation requires `allow_rotation = true`.
pub fn pin_publisher_key(
    publisher: &str,
    public_key_b64: &str,
    allow_rotation: bool,
) -> Result<String, String> {
    let publisher = publisher.trim();
    if publisher.is_empty() {
        return Err("publisher name required".into());
    }
    // Validate key format.
    let _ = key_id_from_public_key_b64(public_key_b64)?;
    let mut pins = read_pins();
    if let Some(existing) = pins.pins.get(publisher) {
        if existing == public_key_b64.trim() {
            return Ok(format!(
                "TOFU pin unchanged for publisher '{publisher}' (key_id={})",
                key_id_from_public_key_b64(public_key_b64)?
            ));
        }
        if !allow_rotation {
            return Err(format!(
                "publisher '{publisher}' already pinned to a different key; pass --rotate to confirm key rotation"
            ));
        }
    }
    pins.pins
        .insert(publisher.to_string(), public_key_b64.trim().to_string());
    write_pins(&pins)?;
    Ok(format!(
        "TOFU pinned publisher '{publisher}' key_id={}",
        key_id_from_public_key_b64(public_key_b64)?
    ))
}

/// Enforce TOFU: pinned key must match manifest (or pin on first sight).
pub fn enforce_tofu(
    verified: &VerifiedSignedTap,
    allow_rotation: bool,
) -> Result<String, String> {
    let pins = read_pins();
    match pins.pins.get(&verified.publisher) {
        None => pin_publisher_key(&verified.publisher, &verified.public_key_b64, false),
        Some(pinned) if pinned == &verified.public_key_b64 => Ok(format!(
            "TOFU ok for '{}' (key_id={})",
            verified.publisher, verified.key_id
        )),
        Some(_) => {
            if allow_rotation {
                pin_publisher_key(&verified.publisher, &verified.public_key_b64, true)
            } else {
                Err(format!(
                    "TOFU mismatch for publisher '{}': manifest key differs from pin; confirm with --rotate",
                    verified.publisher
                ))
            }
        }
    }
}

/// Load + verify a signed tap manifest from disk.
pub fn load_and_verify_signed_tap_file(path: &Path) -> Result<VerifiedSignedTap, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read signed tap {}: {e}", path.display()))?;
    let manifest: SignedTapManifest =
        serde_json::from_str(&raw).map_err(|e| format!("parse signed tap: {e}"))?;
    verify_signed_manifest(&manifest)
}

/// Process `signed:<path>` tap add: verify, TOFU pin, register community→trusted tap stub.
pub fn add_signed_tap_from_file(
    path: &Path,
    allow_rotation: bool,
) -> Result<String, String> {
    let verified = load_and_verify_signed_tap_file(path)?;
    let tofu = enforce_tofu(&verified, allow_rotation)?;
    let name = format!("signed-{}", verified.publisher);
    super::add_tap(
        &name,
        &format!("signed:{}", path.display()),
        "trusted",
    );
    Ok(format!(
        "Added signed tap '{name}' for publisher '{}' (key_id={}).\n{tofu}\n\
         Skills in manifest: {}\n\
         Install still verifies content sha256 before commit.",
        verified.publisher,
        verified.key_id,
        verified.skills.keys().cloned().collect::<Vec<_>>().join(", ")
    ))
}

/// Hex encode (local — avoid new dep).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn test_key() -> SigningKey {
        let mut secret = [0u8; 32];
        secret[0] = 0x42;
        secret[1] = 0x11;
        SigningKey::from_bytes(&secret)
    }

    fn sample_manifest(
        signing_key: &SigningKey,
        publisher: &str,
        content: &str,
    ) -> SignedTapManifest {
        let hash = content_sha256_hex(content.as_bytes());
        let draft = SignedTapManifest {
            schema: SIGNED_TAP_SCHEMA.into(),
            publisher: publisher.into(),
            public_key_b64: String::new(),
            skills: vec![SignedSkillEntry {
                name: "demo".into(),
                content_sha256: hash,
                path: "SKILL.md".into(),
            }],
            signature_b64: String::new(),
        };
        sign_manifest(draft, signing_key).unwrap()
    }

    #[test]
    #[serial]
    fn valid_manifest_verifies_and_hash_matches() {
        let home = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path());
        }
        let key = test_key();
        let content = "---\nname: demo\n---\n# Demo\n";
        let manifest = sample_manifest(&key, "acme-valid", content);
        let verified = verify_signed_manifest(&manifest).unwrap();
        assert_eq!(verified.publisher, "acme-valid");
        assert_content_hash(&verified, "demo", content.as_bytes()).unwrap();
        let msg = enforce_tofu(&verified, false).unwrap();
        assert!(msg.contains("TOFU"));
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    fn bad_signature_fails_closed() {
        let key = test_key();
        let mut manifest = sample_manifest(&key, "acme-badsig", "# hi\n");
        manifest.signature_b64 = B64.encode([0u8; 64]);
        let err = verify_signed_manifest(&manifest).unwrap_err();
        assert!(err.contains("fail closed") || err.contains("verification failed"));
    }

    #[test]
    fn content_hash_mismatch_fails_closed() {
        let key = test_key();
        let manifest = sample_manifest(&key, "acme-hash", "# original\n");
        let verified = verify_signed_manifest(&manifest).unwrap();
        let err = assert_content_hash(&verified, "demo", b"# tampered\n").unwrap_err();
        assert!(err.contains("hash mismatch"));
    }

    #[test]
    #[serial]
    fn tofu_rotation_requires_confirm() {
        let home = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path());
        }
        let key1 = test_key();
        let mut secret2 = [0u8; 32];
        secret2[0] = 0x99;
        let key2 = SigningKey::from_bytes(&secret2);
        let m1 = sample_manifest(&key1, "acme-rotate", "# a\n");
        let v1 = verify_signed_manifest(&m1).unwrap();
        enforce_tofu(&v1, false).unwrap();

        let m2 = sample_manifest(&key2, "acme-rotate", "# a\n");
        let v2 = verify_signed_manifest(&m2).unwrap();
        let err = enforce_tofu(&v2, false).unwrap_err();
        assert!(err.contains("--rotate") || err.contains("mismatch"));
        let ok = enforce_tofu(&v2, true).unwrap();
        assert!(ok.contains("pinned") || ok.contains("TOFU"));
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    #[serial]
    fn add_signed_tap_registers_trusted() {
        let home = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path());
        }
        let key = test_key();
        let manifest = sample_manifest(&key, "acme-add", "# skill\n");
        let path = home.path().join("acme.signed.json");
        std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        let msg = add_signed_tap_from_file(&path, false).unwrap();
        assert!(msg.contains("signed-acme-add"));
        let taps = super::super::read_taps();
        assert!(
            taps.iter()
                .any(|t| t.name == "signed-acme-add" && t.trust_level == "trusted")
        );
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }
}
