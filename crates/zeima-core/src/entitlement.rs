//! Entitlement tokens (SPEC §2): what the user may use, signed by the server
//! with Ed25519 so this device can check it offline against a key built into
//! the app.
//!
//! A token is a compact JWS: `header.payload.signature`, each part
//! base64url without padding. The header is
//! `{"alg":"EdDSA","typ":"zeima-entitlement+jwt"}`; the payload is [`Claims`].
//! Times are unix seconds.
//!
//! Mirrors `server/src/licensing/entitlement.ts`; both run the vectors in
//! `packages/shared-fixtures/entitlement/vectors.json`.
//!
//! Features are gated by name (`has_feature("sync")`), never by plan, so
//! plans can be repackaged without changing the app. **Free never expires
//! and never locks data**: a lapsed token only removes its features.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
pub use ed25519_dalek::VerifyingKey;
use ed25519_dalek::{Signature, Verifier};
use serde::{Deserialize, Serialize};

pub const TOKEN_TYPE: &str = "zeima-entitlement+jwt";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    pub v: u32,
    /// The user the token was issued for.
    pub sub: String,
    #[serde(default)]
    pub org_id: Option<String>,
    pub plan: String,
    #[serde(default)]
    pub seats: Option<u32>,
    pub features: Vec<String>,
    pub iat: i64,
    /// End of the paid period.
    pub exp: i64,
    /// Days the features keep working after `exp` while the server cannot be reached.
    #[serde(default)]
    pub grace_days: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Valid,
    /// Past the paid period but inside the grace period.
    Grace,
    /// Lapsed: Free.
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyError {
    Malformed,
    BadHeader,
    BadPayload,
    BadSignature,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "not an entitlement token",
            Self::BadHeader => "unsupported token type",
            Self::BadPayload => "unreadable token contents",
            Self::BadSignature => "the signature is not valid",
        })
    }
}

impl std::error::Error for VerifyError {}

/// What a verified token allows at a given time.
#[derive(Debug, Clone, PartialEq)]
pub struct Entitlement {
    pub status: Status,
    /// `free` once lapsed.
    pub plan: String,
    pub features: Vec<String>,
    pub org_id: Option<String>,
    pub seats: Option<u32>,
    pub claims: Claims,
}

impl Entitlement {
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// The Free plan: nothing extra, never expires.
pub fn has_feature(entitlement: Option<&Entitlement>, feature: &str) -> bool {
    entitlement.is_some_and(|e| e.has_feature(feature))
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    typ: String,
}

fn decode(part: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(part).ok()
}

/// A trusted key from its 32 raw bytes in base64url.
pub fn parse_key(raw: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = decode(raw)?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Checks the signature against the trusted keys and works out what the
/// token allows at `now`.
pub fn verify(token: &str, trusted: &[VerifyingKey], now: i64) -> Result<Entitlement, VerifyError> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(VerifyError::Malformed);
    };
    let header: Header = decode(h)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or(VerifyError::BadHeader)?;
    if header.alg != "EdDSA" || header.typ != TOKEN_TYPE {
        return Err(VerifyError::BadHeader);
    }
    let signature = decode(s)
        .and_then(|b| Signature::from_slice(&b).ok())
        .ok_or(VerifyError::BadSignature)?;
    let signing = format!("{h}.{p}");
    if !trusted
        .iter()
        .any(|k| k.verify(signing.as_bytes(), &signature).is_ok())
    {
        return Err(VerifyError::BadSignature);
    }
    let claims: Claims = decode(p)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or(VerifyError::BadPayload)?;
    if claims.v != 1 || claims.sub.is_empty() || claims.plan.is_empty() {
        return Err(VerifyError::BadPayload);
    }

    let grace_end = claims.exp + claims.grace_days.unwrap_or(0) * 86_400;
    if now > grace_end {
        return Ok(Entitlement {
            status: Status::Expired,
            plan: "free".into(),
            features: Vec::new(),
            org_id: None,
            seats: None,
            claims,
        });
    }
    Ok(Entitlement {
        status: if now > claims.exp {
            Status::Grace
        } else {
            Status::Valid
        },
        plan: claims.plan.clone(),
        features: claims.features.clone(),
        org_id: claims.org_id.clone(),
        seats: claims.seats,
        claims,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../../packages/shared-fixtures/entitlement/vectors.json");

    #[derive(Deserialize)]
    struct Vectors {
        now: i64,
        #[serde(rename = "trustedKeys")]
        trusted_keys: Vec<String>,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        token: String,
        expect: Expect,
    }

    #[derive(Deserialize)]
    struct Expect {
        status: String,
        plan: Option<String>,
        features: Option<Vec<String>>,
        error: Option<String>,
        #[serde(rename = "orgId")]
        org_id: Option<String>,
        seats: Option<u32>,
    }

    #[test]
    fn shared_vectors() {
        let vectors: Vectors = serde_json::from_str(VECTORS).expect("vectors.json");
        let trusted: Vec<VerifyingKey> = vectors
            .trusted_keys
            .iter()
            .map(|k| parse_key(k).expect("key"))
            .collect();
        assert!(vectors.cases.len() > 5);
        for case in vectors.cases {
            let result = verify(&case.token, &trusted, vectors.now);
            match (case.expect.status.as_str(), result) {
                ("invalid", Err(e)) => {
                    let name = serde_json::to_value(e).unwrap();
                    assert_eq!(name.as_str(), case.expect.error.as_deref(), "{}", case.name);
                }
                ("invalid", Ok(_)) => panic!("{}: accepted", case.name),
                (_, Err(e)) => panic!("{}: refused ({e})", case.name),
                (status, Ok(e)) => {
                    let seen = serde_json::to_value(e.status).unwrap();
                    assert_eq!(seen.as_str(), Some(status), "{}", case.name);
                    assert_eq!(Some(&e.plan), case.expect.plan.as_ref(), "{}: plan", case.name);
                    assert_eq!(
                        Some(&e.features),
                        case.expect.features.as_ref(),
                        "{}: features",
                        case.name
                    );
                    if case.expect.org_id.is_some() {
                        assert_eq!(e.org_id, case.expect.org_id, "{}: org", case.name);
                        assert_eq!(e.seats, case.expect.seats, "{}: seats", case.name);
                    }
                }
            }
        }
    }

    #[test]
    fn features_are_checked_by_name() {
        let vectors: Vectors = serde_json::from_str(VECTORS).unwrap();
        let trusted: Vec<_> = vectors
            .trusted_keys
            .iter()
            .map(|k| parse_key(k).unwrap())
            .collect();
        let pro = verify(&vectors.cases[0].token, &trusted, vectors.now).unwrap();
        assert!(has_feature(Some(&pro), "sync"));
        assert!(!has_feature(Some(&pro), "presence"));
        assert!(!has_feature(None, "sync"), "no token is Free");
    }

    #[test]
    fn a_wrong_length_key_is_not_trusted() {
        assert!(parse_key("AAAA").is_none());
        assert!(parse_key("not base64!").is_none());
    }
}
