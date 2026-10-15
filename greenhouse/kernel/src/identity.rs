// src/identity.rs — the signed identity header (agent.md §6). Ed25519 keys
// derived from KERNEL_MASTER_KEY via HKDF-SHA256 — deterministic, so nothing
// private is ever persisted, and the same master key yields the same keypair
// across restarts (and across the Node reference implementation).
//
// Token envelope remains "v1.<b64url payload JSON>.<b64url ed25519 sig>" so
// a kernel-first rolling upgrade remains wire-compatible with v1 verifiers.
// The claims schema is v3: { v, s: typed subject, u: display/audit identity,
//           a: exact app audience, p: endpoint purpose, g: [effects], iat, exp }

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

// SPKI DER prefix for a raw 32-byte Ed25519 public key (RFC 8410 encoding).
const SPKI_ED25519_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub v: u32,
    pub s: IdentitySubjectKind,
    pub u: String,
    pub a: String,
    pub p: IdentityPurpose,
    pub g: Vec<String>,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySubjectKind {
    Human,
    Scheduler,
    EventService,
    // Autonomous kernel service that reconciles the serving-state cache
    // (router-b2-kernel-native.md §3.2) — not a human, scheduler, or the event
    // bus. Kept a distinct subject so the serving_state purpose is a precise
    // least-privilege pairing rather than borrowing another system caller.
    ServingState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentitySubject {
    Human(String),
    Scheduler,
    EventService,
    ServingState,
}

impl IdentitySubject {
    fn claims(&self) -> (IdentitySubjectKind, String) {
        match self {
            IdentitySubject::Human(user) => (IdentitySubjectKind::Human, user.clone()),
            IdentitySubject::Scheduler => {
                (IdentitySubjectKind::Scheduler, "system:scheduler".into())
            }
            IdentitySubject::EventService => {
                (IdentitySubjectKind::EventService, "system:events".into())
            }
            IdentitySubject::ServingState => (
                IdentitySubjectKind::ServingState,
                "system:serving-state".into(),
            ),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityPurpose {
    Tool,
    Data,
    Event,
    // The kernel→node-monitor serving-state reconcile call (§3.2). Its own
    // purpose so the additive interim `Data`/`read` authorization can be
    // retired the moment the kernel can mint and use this end-to-end.
    ServingState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    Invalid,
    Audience,
    Purpose,
    Effect,
    Subject,
}

pub struct Identity {
    signing: SigningKey,
    pub public_key_pem: String,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub fn create_identity(master: &[u8; 32]) -> Identity {
    let hk = Hkdf::<Sha256>::new(Some(b"greenhouse-kernel"), master);
    let mut seed = [0u8; 32];
    hk.expand(b"identity-ed25519-v1", &mut seed)
        .expect("hkdf expand");
    let signing = SigningKey::from_bytes(&seed);

    let mut der = SPKI_ED25519_PREFIX.to_vec();
    der.extend_from_slice(&signing.verifying_key().to_bytes());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&der);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let public_key_pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    );

    Identity {
        signing,
        public_key_pem,
    }
}

impl Identity {
    pub fn mint(
        &self,
        subject: IdentitySubject,
        app: &str,
        purpose: IdentityPurpose,
        grants: &[String],
        ttl_sec: i64,
    ) -> String {
        let iat = now();
        let (subject_kind, user) = subject.claims();
        let claims = Claims {
            v: 3,
            s: subject_kind,
            u: user,
            a: app.into(),
            p: purpose,
            g: grants.to_vec(),
            iat,
            exp: iat + ttl_sec,
        };
        let payload = B64.encode(serde_json::to_vec(&claims).unwrap());
        let sig = self.signing.sign(payload.as_bytes());
        format!("v1.{payload}.{}", B64.encode(sig.to_bytes()))
    }

    pub fn verify(&self, token: &str) -> Option<Claims> {
        verify_with_key(&self.signing.verifying_key(), token)
    }
}

fn verify_with_key(key: &VerifyingKey, token: &str) -> Option<Claims> {
    let mut parts = token.splitn(3, '.');
    let (v, payload, sig) = (parts.next()?, parts.next()?, parts.next()?);
    if v != "v1" {
        return None;
    }
    let sig = Signature::from_slice(&B64.decode(sig).ok()?).ok()?;
    key.verify(payload.as_bytes(), &sig).ok()?;
    let claims: Claims = serde_json::from_slice(&B64.decode(payload).ok()?).ok()?;
    if claims.v != 3 || claims.exp < now() {
        return None;
    }
    Some(claims)
}

// For apps (and tests): validate a token against the kernel's published PEM.
pub fn verify_with_pem(pem: &str, token: &str) -> Option<Claims> {
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let raw: [u8; 32] = der.get(der.len().checked_sub(32)?..)?.try_into().ok()?;
    let key = VerifyingKey::from_bytes(&raw).ok()?;
    verify_with_key(&key, token)
}

/// Shared app-side enforcement for every privileged endpoint. Raw signature
/// verification is intentionally not enough: exact audience, endpoint
/// purpose, and the required effect are part of the authorization contract.
pub fn verify_app_request(
    pem: &str,
    token: &str,
    app_id: &str,
    purpose: IdentityPurpose,
    required_effect: Option<&str>,
) -> Result<Claims, IdentityError> {
    let claims = verify_with_pem(pem, token).ok_or(IdentityError::Invalid)?;
    if claims.a != app_id {
        return Err(IdentityError::Audience);
    }
    if claims.p != purpose {
        return Err(IdentityError::Purpose);
    }
    let subject_allowed = match purpose {
        IdentityPurpose::Tool => matches!(
            claims.s,
            IdentitySubjectKind::Human | IdentitySubjectKind::Scheduler
        ),
        IdentityPurpose::Data => claims.s == IdentitySubjectKind::Human,
        IdentityPurpose::Event => claims.s == IdentitySubjectKind::EventService,
        IdentityPurpose::ServingState => claims.s == IdentitySubjectKind::ServingState,
    };
    if !subject_allowed {
        return Err(IdentityError::Subject);
    }
    if required_effect.is_some_and(|effect| !claims.g.iter().any(|grant| grant == effect)) {
        return Err(IdentityError::Effect);
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master() -> [u8; 32] {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).unwrap();
        k
    }

    #[test]
    fn mint_verify_roundtrip() {
        let id = create_identity(&master());
        let token = id.mint(
            IdentitySubject::Human("owner".into()),
            "finance",
            IdentityPurpose::Tool,
            &["read".into(), "write".into()],
            300,
        );
        let c = id.verify(&token).unwrap();
        assert_eq!(c.u, "owner");
        assert_eq!(c.s, IdentitySubjectKind::Human);
        assert_eq!(c.a, "finance");
        assert_eq!(c.p, IdentityPurpose::Tool);
        assert_eq!(c.g, vec!["read", "write"]);
        assert!(c.exp > c.iat);
    }

    #[test]
    fn tampered_payload_fails() {
        let id = create_identity(&master());
        let token = id.mint(
            IdentitySubject::Human("guest".into()),
            "finance",
            IdentityPurpose::Tool,
            &["read".into()],
            300,
        );
        let parts: Vec<&str> = token.split('.').collect();
        let mut claims: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .unwrap(),
        )
        .unwrap();
        claims["g"] = serde_json::json!(["read", "write", "command"]); // escalation attempt
        let forged = format!(
            "{}.{}.{}",
            parts[0],
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string()),
            parts[2]
        );
        assert!(id.verify(&forged).is_none());
    }

    #[test]
    fn expired_fails() {
        let id = create_identity(&master());
        assert!(id
            .verify(&id.mint(
                IdentitySubject::Human("owner".into()),
                "x",
                IdentityPurpose::Tool,
                &[],
                -10
            ))
            .is_none());
    }

    #[test]
    fn different_master_cannot_verify() {
        let a = create_identity(&master());
        let b = create_identity(&master());
        assert!(b
            .verify(&a.mint(
                IdentitySubject::Human("owner".into()),
                "x",
                IdentityPurpose::Tool,
                &[],
                300
            ))
            .is_none());
    }

    #[test]
    fn deterministic_across_restarts() {
        let k = master();
        let a = create_identity(&k);
        let b = create_identity(&k);
        assert_eq!(a.public_key_pem, b.public_key_pem);
        assert!(b
            .verify(&a.mint(
                IdentitySubject::Human("owner".into()),
                "x",
                IdentityPurpose::Tool,
                &[],
                300
            ))
            .is_some());
    }

    #[test]
    fn pem_verification_matches() {
        let id = create_identity(&master());
        let token = id.mint(
            IdentitySubject::Human("owner".into()),
            "stub",
            IdentityPurpose::Tool,
            &["read".into()],
            300,
        );
        let c = verify_with_pem(&id.public_key_pem, &token).unwrap();
        assert_eq!(c.u, "owner");
    }

    #[test]
    fn garbage_returns_none() {
        let id = create_identity(&master());
        for t in ["", "v1", "v1.x", "v1.x.y", "v2.a.b"] {
            assert!(id.verify(t).is_none(), "{t}");
        }
    }

    #[test]
    fn app_request_requires_exact_audience_purpose_effect_and_event_caller() {
        let id = create_identity(&master());
        let read = vec!["read".into()];
        let tool = id.mint(
            IdentitySubject::Human("owner".into()),
            "app-a",
            IdentityPurpose::Tool,
            &read,
            300,
        );
        let event = id.mint(
            IdentitySubject::EventService,
            "app-a",
            IdentityPurpose::Event,
            &[],
            300,
        );
        let user_event = id.mint(
            IdentitySubject::Human("system:events".into()),
            "app-a",
            IdentityPurpose::Event,
            &read,
            300,
        );
        let pem = &id.public_key_pem;

        assert!(
            verify_app_request(pem, &tool, "app-a", IdentityPurpose::Tool, Some("read")).is_ok()
        );
        assert_eq!(
            verify_app_request(pem, &tool, "app-b", IdentityPurpose::Tool, Some("read")),
            Err(IdentityError::Audience)
        );
        assert_eq!(
            verify_app_request(pem, &event, "app-a", IdentityPurpose::Tool, Some("read")),
            Err(IdentityError::Purpose)
        );
        assert_eq!(
            verify_app_request(pem, &tool, "app-a", IdentityPurpose::Event, None),
            Err(IdentityError::Purpose)
        );
        assert_eq!(
            verify_app_request(pem, &user_event, "app-a", IdentityPurpose::Event, None),
            Err(IdentityError::Subject)
        );
    }

    #[test]
    fn serving_state_purpose_is_a_distinct_least_privilege_pairing() {
        let id = create_identity(&master());
        let pem = &id.public_key_pem;
        let serving = id.mint(
            IdentitySubject::ServingState,
            "node-monitor",
            IdentityPurpose::ServingState,
            &[],
            300,
        );
        // The dedicated serving_state identity is accepted for that purpose.
        assert!(verify_app_request(
            pem,
            &serving,
            "node-monitor",
            IdentityPurpose::ServingState,
            None
        )
        .is_ok());
        // The interim Data/read identity no longer reaches a serving_state endpoint.
        let data = id.mint(
            IdentitySubject::Human("owner".into()),
            "node-monitor",
            IdentityPurpose::Data,
            &["read".into()],
            300,
        );
        assert_eq!(
            verify_app_request(
                pem,
                &data,
                "node-monitor",
                IdentityPurpose::ServingState,
                None
            ),
            Err(IdentityError::Purpose)
        );
        // The serving_state subject cannot be replayed into user data/tool APIs.
        assert_eq!(
            verify_app_request(
                pem,
                &serving,
                "node-monitor",
                IdentityPurpose::Data,
                Some("read")
            ),
            Err(IdentityError::Purpose)
        );
        // A human cannot assume the serving_state purpose (subject gate).
        let human_serving = id.mint(
            IdentitySubject::Human("owner".into()),
            "node-monitor",
            IdentityPurpose::ServingState,
            &[],
            300,
        );
        assert_eq!(
            verify_app_request(
                pem,
                &human_serving,
                "node-monitor",
                IdentityPurpose::ServingState,
                None
            ),
            Err(IdentityError::Subject)
        );
    }
}
