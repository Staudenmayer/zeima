//! Addon signatures (docs/addons.md, "Signing"): a publisher signs what the
//! addon *is* (its manifest and its module) with Ed25519, and the app checks
//! the signature against the keys it trusts before it tells the person who
//! made the addon.
//!
//! The package carries `addon.sig`, a small JSON file `{"keyId": "...",
//! "signature": "<base64url>"}`. The signed message is
//!
//! ```text
//! zeima-addon-v1\n<sha256 of addon.toml, hex>\n<sha256 of the module, hex>\n
//! ```
//!
//! so the signature does not depend on how the files were zipped. Mirrors
//! `scripts/addons.mjs`; both run `packages/shared-fixtures/addons/vectors.json`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SIGNATURE_FILE: &str = "addon.sig";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureFile {
    pub key_id: String,
    pub signature: String,
}

/// What the person is told about who made an addon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SignatureStatus {
    /// No signature.
    Unsigned,
    /// Signed by a key the app trusts, and the files match.
    Trusted,
    /// Signed, but by a key the app does not know.
    UnknownKey,
    /// Signed, but the files do not match the signature (changed after signing, or damaged).
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SignatureCheck {
    pub status: SignatureStatus,
    pub key_id: Option<String>,
    /// The publisher's name, for a trusted key.
    pub signer: Option<String>,
}

impl SignatureCheck {
    fn of(status: SignatureStatus, key_id: Option<String>, signer: Option<String>) -> Self {
        Self { status, key_id, signer }
    }

    pub fn unsigned() -> Self {
        Self::of(SignatureStatus::Unsigned, None, None)
    }

    /// Whether the signature is good (a trusted publisher made exactly these files).
    pub fn is_trusted(&self) -> bool {
        self.status == SignatureStatus::Trusted
    }
}

/// A publisher key the app trusts.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    pub key_id: String,
    pub name: String,
    pub key: VerifyingKey,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bytes that are signed.
pub fn message(manifest_src: &[u8], wasm: &[u8]) -> Vec<u8> {
    format!(
        "zeima-addon-v1\n{}\n{}\n",
        hex(&Sha256::digest(manifest_src)),
        hex(&Sha256::digest(wasm))
    )
    .into_bytes()
}

/// A raw 32-byte Ed25519 public key in base64url.
pub fn parse_public_key(raw: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(raw.trim()).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Checks the package's signature file (if any) against the trusted keys.
pub fn check(manifest_src: &[u8], wasm: &[u8], signature_json: Option<&str>, trusted: &[TrustedKey]) -> SignatureCheck {
    let Some(raw) = signature_json else {
        return SignatureCheck::unsigned();
    };
    let Ok(file) = serde_json::from_str::<SignatureFile>(raw) else {
        return SignatureCheck::of(SignatureStatus::Invalid, None, None);
    };
    let Some(key) = trusted.iter().find(|k| k.key_id == file.key_id) else {
        return SignatureCheck::of(SignatureStatus::UnknownKey, Some(file.key_id), None);
    };
    let good = URL_SAFE_NO_PAD
        .decode(file.signature.trim())
        .ok()
        .and_then(|b| Signature::from_slice(&b).ok())
        .is_some_and(|sig| key.key.verify(&message(manifest_src, wasm), &sig).is_ok());
    let status = if good { SignatureStatus::Trusted } else { SignatureStatus::Invalid };
    SignatureCheck::of(status, Some(key.key_id.clone()), good.then(|| key.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Vectors {
        key: VectorKey,
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct VectorKey {
        key_id: String,
        name: String,
        public_key: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        name: String,
        manifest: String,
        wasm_base64: String,
        signature: Option<serde_json::Value>,
        expect: String,
    }

    #[test]
    fn shared_vectors() {
        let v: Vectors = serde_json::from_str(include_str!("../../../packages/shared-fixtures/addons/vectors.json")).unwrap();
        let trusted = [TrustedKey {
            key_id: v.key.key_id.clone(),
            name: v.key.name.clone(),
            key: parse_public_key(&v.key.public_key).expect("key parses"),
        }];
        for c in v.cases {
            let wasm = base64::engine::general_purpose::STANDARD.decode(&c.wasm_base64).unwrap();
            let sig = c.signature.as_ref().map(|s| s.to_string());
            let got = check(c.manifest.as_bytes(), &wasm, sig.as_deref(), &trusted);
            let want: SignatureStatus = serde_json::from_value(serde_json::Value::String(c.expect.clone())).unwrap();
            assert_eq!(got.status, want, "{}", c.name);
            assert_eq!(got.signer.is_some(), want == SignatureStatus::Trusted, "{}", c.name);
        }
    }

    #[test]
    fn a_malformed_signature_file_is_invalid() {
        assert_eq!(check(b"m", b"w", Some("not json"), &[]).status, SignatureStatus::Invalid);
        assert_eq!(check(b"m", b"w", None, &[]).status, SignatureStatus::Unsigned);
    }
}
