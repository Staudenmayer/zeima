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
    /// Signed by a publisher key built into the app (the vendor's), and the files match.
    Trusted,
    /// Signed by a publisher the connected server vouches for: as far as the installation's administrator is
    /// trusted, not more. Shown differently from the vendor's.
    ServerTrusted,
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
    /// SHA-256 (hex) of the public key that verified the signature. Set only when the signature verified; `key_id`
    /// is a label copied from the package and proves nothing by itself.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

impl SignatureCheck {
    fn of(
        status: SignatureStatus,
        key_id: Option<String>,
        signer: Option<String>,
        fingerprint: Option<String>,
    ) -> Self {
        Self {
            status,
            key_id,
            signer,
            fingerprint,
        }
    }

    pub fn unsigned() -> Self {
        Self::of(SignatureStatus::Unsigned, None, None, None)
    }

    /// Whether `self` (a package being installed) may replace an installed add-on whose recorded signature is
    /// `installed`. The publisher is identified by the verified key, not by the key id string. An installed
    /// add-on without a verified signature has no publisher to continue; one recorded before fingerprints
    /// existed needs a verified signature with the same key id.
    pub fn continues(&self, installed: &SignatureCheck) -> bool {
        match (&installed.fingerprint, installed.is_trusted()) {
            (Some(old), _) => self.is_trusted() && self.fingerprint.as_ref() == Some(old),
            (None, true) => self.is_trusted() && self.key_id.is_some() && self.key_id == installed.key_id,
            (None, false) => true,
        }
    }

    /// Whether the signature is good: a publisher the app or its server trusts made exactly these files.
    pub fn is_trusted(&self) -> bool {
        matches!(
            self.status,
            SignatureStatus::Trusted | SignatureStatus::ServerTrusted
        )
    }
}

/// A publisher key the app trusts.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    pub key_id: String,
    pub name: String,
    pub key: VerifyingKey,
    /// Built into the app (the vendor's) rather than handed out by the connected server.
    pub vendor: bool,
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
    // A small-order key verifies signatures nobody made.
    VerifyingKey::from_bytes(&bytes).ok().filter(|k| !k.is_weak())
}

/// Checks the package's signature file (if any) against the trusted keys.
pub fn check(
    manifest_src: &[u8],
    wasm: &[u8],
    signature_json: Option<&str>,
    trusted: &[TrustedKey],
) -> SignatureCheck {
    let Some(raw) = signature_json else {
        return SignatureCheck::unsigned();
    };
    let Ok(file) = serde_json::from_str::<SignatureFile>(raw) else {
        return SignatureCheck::of(SignatureStatus::Invalid, None, None, None);
    };
    let Some(key) = trusted.iter().find(|k| k.key_id == file.key_id) else {
        return SignatureCheck::of(SignatureStatus::UnknownKey, Some(file.key_id), None, None);
    };
    let good = URL_SAFE_NO_PAD
        .decode(file.signature.trim())
        .ok()
        .and_then(|b| Signature::from_slice(&b).ok())
        .is_some_and(|sig| key.key.verify_strict(&message(manifest_src, wasm), &sig).is_ok());
    let status = match (good, key.vendor) {
        (true, true) => SignatureStatus::Trusted,
        (true, false) => SignatureStatus::ServerTrusted,
        (false, _) => SignatureStatus::Invalid,
    };
    SignatureCheck::of(
        status,
        Some(key.key_id.clone()),
        good.then(|| key.name.clone()),
        good.then(|| hex(&Sha256::digest(key.key.as_bytes()))),
    )
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
        let v: Vectors = serde_json::from_str(include_str!(
            "../../../packages/shared-fixtures/addons/vectors.json"
        ))
        .unwrap();
        let trusted = [TrustedKey {
            key_id: v.key.key_id.clone(),
            name: v.key.name.clone(),
            key: parse_public_key(&v.key.public_key).expect("key parses"),
            vendor: true,
        }];
        for c in v.cases {
            let wasm = base64::engine::general_purpose::STANDARD
                .decode(&c.wasm_base64)
                .unwrap();
            let sig = c.signature.as_ref().map(|s| s.to_string());
            let got = check(c.manifest.as_bytes(), &wasm, sig.as_deref(), &trusted);
            let want: SignatureStatus =
                serde_json::from_value(serde_json::Value::String(c.expect.clone())).unwrap();
            assert_eq!(got.status, want, "{}", c.name);
            assert_eq!(
                got.signer.is_some(),
                want == SignatureStatus::Trusted,
                "{}",
                c.name
            );
            // The same files signed by a key the server handed out are "server trusted", never vendor trusted.
            let by_server = [TrustedKey {
                vendor: false,
                ..trusted[0].clone()
            }];
            let got = check(c.manifest.as_bytes(), &wasm, sig.as_deref(), &by_server);
            let want_server = if want == SignatureStatus::Trusted {
                SignatureStatus::ServerTrusted
            } else {
                want
            };
            assert_eq!(got.status, want_server, "{} (server key)", c.name);
        }
    }

    #[test]
    fn a_malformed_signature_file_is_invalid() {
        assert_eq!(
            check(b"m", b"w", Some("not json"), &[]).status,
            SignatureStatus::Invalid
        );
        assert_eq!(check(b"m", b"w", None, &[]).status, SignatureStatus::Unsigned);
    }

    fn signed_by(seed: u8, key_id: &str, vendor: bool, claim: &str) -> SignatureCheck {
        use ed25519_dalek::{Signer, SigningKey};
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let sig = URL_SAFE_NO_PAD.encode(sk.sign(&message(b"m", b"w")).to_bytes());
        let file = format!("{{\"keyId\":\"{claim}\",\"signature\":\"{sig}\"}}");
        let trusted = [TrustedKey {
            key_id: key_id.into(),
            name: "P".into(),
            key: sk.verifying_key(),
            vendor,
        }];
        check(b"m", b"w", Some(&file), &trusted)
    }

    #[test]
    fn publisher_continuity_is_bound_to_the_verified_key() {
        let installed = signed_by(1, "pub", true, "pub");
        assert!(installed.fingerprint.is_some());
        // Same key: passes, also when the server (not the vendor) vouches for it now.
        assert!(signed_by(1, "pub", true, "pub").continues(&installed));
        assert!(signed_by(1, "pub", false, "pub").continues(&installed));
        // (a) The key is not known: only the claimed id matches.
        let unknown = check(
            b"m",
            b"w",
            Some(r#"{"keyId":"pub","signature":"AAAA"}"#),
            &[],
        );
        assert_eq!(unknown.status, SignatureStatus::UnknownKey);
        assert!(!unknown.continues(&installed));
        // (b) The id now belongs to a different, server-published key.
        assert!(!signed_by(2, "pub", false, "pub").continues(&installed));
        // (c) Signed by a different key under another id.
        assert!(!signed_by(3, "other", true, "other").continues(&installed));
        assert!(!SignatureCheck::unsigned().continues(&installed));
    }

    #[test]
    fn continuity_for_records_without_a_fingerprint() {
        let mut legacy = signed_by(1, "pub", true, "pub");
        legacy.fingerprint = None;
        let mut same_id = signed_by(2, "pub", true, "pub");
        assert!(same_id.continues(&legacy));
        same_id.key_id = Some("x".into());
        assert!(!same_id.continues(&legacy));
        let unknown = SignatureCheck::of(SignatureStatus::UnknownKey, Some("pub".into()), None, None);
        assert!(!unknown.continues(&legacy));
        // An installed unknown-key or unsigned record has no verified publisher to continue.
        assert!(signed_by(1, "pub", true, "pub").continues(&unknown));
        assert!(signed_by(1, "pub", true, "pub").continues(&SignatureCheck::unsigned()));
    }
}
