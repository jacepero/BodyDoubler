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
        // Stand-in: the rest of this test was not captured. It swaps the
        // person in the payload and keeps the old signature.
        let parts: Vec<&str> = token.split('.').collect();
        let payload = String::from_utf8(B64.decode(parts[1]).unwrap()).unwrap();
        let forged = B64.encode(payload.replace("\"guest\"", "\"owner\""));
        assert_ne!(forged, parts[1]);
        assert!(id
            .verify(&format!("{}.{forged}.{}", parts[0], parts[2]))
            .is_none());
    }

    // Stand-in: the tests below are reconstructed. They check the behaviour
    // apps rely on through verify_with_pem and verify_app_request.

    fn tool_token(id: &Identity, subject: IdentitySubject, grants: &[&str]) -> String {
        let grants: Vec<String> = grants.iter().map(|g| g.to_string()).collect();
        id.mint(subject, "weather", IdentityPurpose::Tool, &grants, 300)
    }

    #[test]
    fn the_published_pem_verifies_and_another_key_does_not() {
        let id = create_identity(&[7u8; 32]);
        assert!(id
            .public_key_pem
            .starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(id.public_key_pem.ends_with("-----END PUBLIC KEY-----\n"));
        // The same master key gives the same key pair.
        assert_eq!(
            create_identity(&[7u8; 32]).public_key_pem,
            id.public_key_pem
        );

        let token = tool_token(&id, IdentitySubject::Human("owner".into()), &["read"]);
        let claims = verify_with_pem(&id.public_key_pem, &token).unwrap();
        assert_eq!((claims.v, claims.u.as_str()), (3, "owner"));
        assert_eq!(claims.exp - claims.iat, 300);

        let other = create_identity(&[8u8; 32]);
        assert!(verify_with_pem(&other.public_key_pem, &token).is_none());
        assert!(verify_with_pem("not a key", &token).is_none());
    }

    #[test]
    fn the_wire_form_is_v1_payload_signature() {
        let id = create_identity(&master());
        let token = tool_token(&id, IdentitySubject::Human("owner".into()), &["read"]);
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "v1");
        let payload: serde_json::Value =
            serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(payload["s"], "human");
        assert_eq!(payload["a"], "weather");
        assert_eq!(payload["p"], "tool");
        assert_eq!(payload["g"], serde_json::json!(["read"]));
        // Another envelope version is refused.
        let v2 = token.replacen("v1.", "v2.", 1);
        assert!(id.verify(&v2).is_none());
    }

    #[test]
    fn an_expired_token_is_invalid() {
        let id = create_identity(&master());
        let token = id.mint(
            IdentitySubject::Human("owner".into()),
            "weather",
            IdentityPurpose::Tool,
            &["read".into()],
            -1,
        );
        assert!(id.verify(&token).is_none());
        assert_eq!(
            verify_app_request(
                &id.public_key_pem,
                &token,
                "weather",
                IdentityPurpose::Tool,
                None
            ),
            Err(IdentityError::Invalid)
        );
    }

    #[test]
    fn app_requests_check_audience_purpose_subject_and_effect() {
        let id = create_identity(&master());
        let pem = &id.public_key_pem;
        let owner = || IdentitySubject::Human("owner".into());

        let token = tool_token(&id, owner(), &["read", "write"]);
        let claims =
            verify_app_request(pem, &token, "weather", IdentityPurpose::Tool, Some("write"))
                .unwrap();
        assert_eq!(claims.u, "owner");
        assert!(verify_app_request(pem, &token, "weather", IdentityPurpose::Tool, None).is_ok());

        // Garbage and a token for another key are invalid.
        assert_eq!(
            verify_app_request(pem, "v1.e30.AAAA", "weather", IdentityPurpose::Tool, None),
            Err(IdentityError::Invalid)
        );
        let stranger = tool_token(&create_identity(&[9u8; 32]), owner(), &["read"]);
        assert_eq!(
            verify_app_request(pem, &stranger, "weather", IdentityPurpose::Tool, None),
            Err(IdentityError::Invalid)
        );

        // Another app's token.
        assert_eq!(
            verify_app_request(pem, &token, "home-assistant", IdentityPurpose::Tool, None),
            Err(IdentityError::Audience)
        );
        // A tool token at a data endpoint.
        assert_eq!(
            verify_app_request(pem, &token, "weather", IdentityPurpose::Data, None),
            Err(IdentityError::Purpose)
        );
        // Grants are not cumulative: write does not imply command.
        assert_eq!(
            verify_app_request(
                pem,
                &token,
                "weather",
                IdentityPurpose::Tool,
                Some("command")
            ),
            Err(IdentityError::Effect)
        );
        let read_only = tool_token(&id, owner(), &["read"]);
        assert_eq!(
            verify_app_request(
                pem,
                &read_only,
                "weather",
                IdentityPurpose::Tool,
                Some("write")
            ),
            Err(IdentityError::Effect)
        );
    }

    #[test]
    fn each_purpose_admits_only_its_callers() {
        let id = create_identity(&master());
        let pem = &id.public_key_pem;
        let mint = |subject: IdentitySubject, purpose: IdentityPurpose, grants: &[&str]| {
            let grants: Vec<String> = grants.iter().map(|g| g.to_string()).collect();
            id.mint(subject, "weather", purpose, &grants, 300)
        };

        // The scheduler calls command tools.
        let scheduled = mint(
            IdentitySubject::Scheduler,
            IdentityPurpose::Tool,
            &["command"],
        );
        let claims = verify_app_request(
            pem,
            &scheduled,
            "weather",
            IdentityPurpose::Tool,
            Some("command"),
        )
        .unwrap();
        assert_eq!(claims.s, IdentitySubjectKind::Scheduler);
        assert_eq!(claims.u, "system:scheduler");

        // It is never a person reading data.
        let scheduler_data = mint(IdentitySubject::Scheduler, IdentityPurpose::Data, &["read"]);
        assert_eq!(
            verify_app_request(
                pem,
                &scheduler_data,
                "weather",
                IdentityPurpose::Data,
                Some("read")
            ),
            Err(IdentityError::Subject)
        );
        let human_data = mint(
            IdentitySubject::Human("owner".into()),
            IdentityPurpose::Data,
            &["read"],
        );
        assert!(verify_app_request(
            pem,
            &human_data,
            "weather",
            IdentityPurpose::Data,
            Some("read")
        )
        .is_ok());

        // Events come only from the event service, and tools never do.
        let events_tool = mint(
            IdentitySubject::EventService,
            IdentityPurpose::Tool,
            &["read"],
        );
        assert_eq!(
            verify_app_request(pem, &events_tool, "weather", IdentityPurpose::Tool, None),
            Err(IdentityError::Subject)
        );
        let event = mint(IdentitySubject::EventService, IdentityPurpose::Event, &[]);
        let claims =
            verify_app_request(pem, &event, "weather", IdentityPurpose::Event, None).unwrap();
        assert_eq!(claims.u, "system:events");
        let human_event = mint(
            IdentitySubject::Human("owner".into()),
            IdentityPurpose::Event,
            &[],
        );
        assert_eq!(
            verify_app_request(pem, &human_event, "weather", IdentityPurpose::Event, None),
            Err(IdentityError::Subject)
        );

        // The serving-state purpose admits only its own subject.
        let serving = mint(
            IdentitySubject::ServingState,
            IdentityPurpose::ServingState,
            &[],
        );
        assert!(verify_app_request(
            pem,
            &serving,
            "weather",
            IdentityPurpose::ServingState,
            None
        )
        .is_ok());
        let serving_tool = mint(
            IdentitySubject::ServingState,
            IdentityPurpose::Tool,
            &["read"],
        );
        assert_eq!(
            verify_app_request(pem, &serving_tool, "weather", IdentityPurpose::Tool, None),
            Err(IdentityError::Subject)
        );
    }

    #[test]
    fn the_subject_is_checked_before_the_effect() {
        // An event-service token at a tool endpoint is a subject error even
        // when it also lacks the effect.
        let id = create_identity(&master());
        let token = id.mint(
            IdentitySubject::EventService,
            "weather",
            IdentityPurpose::Tool,
            &[],
            300,
        );
        assert_eq!(
            verify_app_request(
                &id.public_key_pem,
                &token,
                "weather",
                IdentityPurpose::Tool,
                Some("read")
            ),
            Err(IdentityError::Subject)
        );
    }
}
