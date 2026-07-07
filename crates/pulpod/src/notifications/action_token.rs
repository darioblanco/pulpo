//! Short-lived, HMAC-signed capability tokens for push notification actions.
//!
//! A service worker cannot read the app's bearer auth token (it only has whatever
//! the push payload carries), so the "Stop session" action button on a
//! `usage_alert` push notification needs its own, narrowly-scoped credential: a
//! token that authorizes exactly one thing — stopping one specific session —
//! and expires quickly. `POST /api/v1/push/action` (see
//! [`crate::api::push::action`]) accepts this token in place of a bearer token;
//! the token itself *is* the capability.
//!
//! Format: `<base64url(json claims)>.<hex hmac-sha256>`, deliberately similar to
//! a minimal JWT. The HMAC covers the base64url-encoded claims exactly as
//! transmitted, so any change to the claims (including a tampered `session_id`)
//! invalidates the signature.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::api::auth::constant_time_eq;

/// The only action currently supported: stop the session.
pub const STOP_ACTION: &str = "stop";

/// Default token lifetime: 30 minutes.
pub const DEFAULT_TTL_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ActionClaims {
    session_id: String,
    action: String,
    /// Expiry as Unix seconds.
    exp: i64,
}

/// Why an action token failed to verify.
///
/// Deliberately not surfaced to callers of the HTTP endpoint (which returns a
/// single generic 401) — this exists for precise unit testing of
/// [`verify_action_token`] only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionTokenError {
    /// Not in `<payload>.<signature>` form, or the payload isn't valid base64url JSON.
    Malformed,
    /// The HMAC signature doesn't match the payload.
    BadSignature,
    /// `exp` is in the past.
    Expired,
    /// The token's `action` claim doesn't match what the caller expected.
    WrongAction,
}

fn compute_hmac(secret: &str, data: &[u8]) -> String {
    // Matches `webhook::compute_signature` — HMAC-SHA256 accepts any key length.
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Sign a new action token binding `session_id` to `action`, expiring `ttl_secs`
/// after `now`.
pub fn sign_action_token(
    secret: &str,
    session_id: &str,
    action: &str,
    now: DateTime<Utc>,
    ttl_secs: i64,
) -> String {
    let claims = ActionClaims {
        session_id: session_id.to_owned(),
        action: action.to_owned(),
        exp: now.timestamp() + ttl_secs,
    };
    // `ActionClaims` is a plain struct of primitives — serialization cannot fail.
    let payload = serde_json::to_vec(&claims).unwrap_or_default();
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload);
    let signature = compute_hmac(secret, payload_b64.as_bytes());
    format!("{payload_b64}.{signature}")
}

/// Verify an action token: signature, expiry, and that its `action` claim
/// matches `expected_action`. Returns the bound `session_id` on success.
pub fn verify_action_token(
    secret: &str,
    token: &str,
    expected_action: &str,
    now: DateTime<Utc>,
) -> Result<String, ActionTokenError> {
    let (payload_b64, signature) = token.split_once('.').ok_or(ActionTokenError::Malformed)?;

    let expected_signature = compute_hmac(secret, payload_b64.as_bytes());
    if !constant_time_eq(&expected_signature, signature) {
        return Err(ActionTokenError::BadSignature);
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| ActionTokenError::Malformed)?;
    let claims: ActionClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| ActionTokenError::Malformed)?;

    if claims.action != expected_action {
        return Err(ActionTokenError::WrongAction);
    }
    if claims.exp < now.timestamp() {
        return Err(ActionTokenError::Expired);
    }

    Ok(claims.session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-08T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let token = sign_action_token("secret", "sess-1", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let session_id = verify_action_token("secret", &token, STOP_ACTION, now()).unwrap();
        assert_eq!(session_id, "sess-1");
    }

    #[test]
    fn test_verify_rejects_wrong_secret() {
        let token = sign_action_token("secret-a", "sess-1", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let err = verify_action_token("secret-b", &token, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::BadSignature);
    }

    #[test]
    fn test_verify_rejects_expired_token() {
        let token = sign_action_token("secret", "sess-1", STOP_ACTION, now(), -1);
        let err = verify_action_token("secret", &token, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::Expired);
    }

    #[test]
    fn test_verify_accepts_token_at_exact_expiry_boundary() {
        // exp == now is not yet expired (only exp < now is rejected).
        let token = sign_action_token("secret", "sess-1", STOP_ACTION, now(), 0);
        assert!(verify_action_token("secret", &token, STOP_ACTION, now()).is_ok());
    }

    #[test]
    fn test_verify_rejects_wrong_action() {
        let token = sign_action_token("secret", "sess-1", "purge", now(), DEFAULT_TTL_SECS);
        let err = verify_action_token("secret", &token, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::WrongAction);
    }

    #[test]
    fn test_verify_rejects_tampered_signature() {
        let token = sign_action_token("secret", "sess-1", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let (payload, _sig) = token.split_once('.').unwrap();
        let tampered =
            format!("{payload}.0000000000000000000000000000000000000000000000000000000000000000");
        let err = verify_action_token("secret", &tampered, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::BadSignature);
    }

    #[test]
    fn test_verify_rejects_tampered_session_id() {
        // Swap in a different session's payload but keep the original signature —
        // simulates an attacker trying to redirect a valid token at another
        // session ("wrong session"). The stale signature no longer matches.
        let token_a = sign_action_token("secret", "sess-a", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let token_b = sign_action_token("secret", "sess-b", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let (payload_b, _) = token_b.split_once('.').unwrap();
        let (_, signature_a) = token_a.split_once('.').unwrap();
        let frankenstein = format!("{payload_b}.{signature_a}");
        let err = verify_action_token("secret", &frankenstein, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::BadSignature);
    }

    #[test]
    fn test_verify_rejects_malformed_no_separator() {
        let err = verify_action_token("secret", "not-a-token", STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::Malformed);
    }

    #[test]
    fn test_verify_rejects_malformed_payload_not_base64() {
        // A valid-looking signature over garbage payload bytes so the signature
        // check passes but base64 decoding then fails.
        let sig = compute_hmac("secret", b"not-base64!!!");
        let token = format!("not-base64!!!.{sig}");
        let err = verify_action_token("secret", &token, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::Malformed);
    }

    #[test]
    fn test_verify_rejects_malformed_payload_not_json() {
        let payload_b64 = URL_SAFE_NO_PAD.encode(b"not json");
        let sig = compute_hmac("secret", payload_b64.as_bytes());
        let token = format!("{payload_b64}.{sig}");
        let err = verify_action_token("secret", &token, STOP_ACTION, now()).unwrap_err();
        assert_eq!(err, ActionTokenError::Malformed);
    }

    #[test]
    fn test_different_sessions_yield_different_tokens() {
        let a = sign_action_token("secret", "sess-a", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        let b = sign_action_token("secret", "sess-b", STOP_ACTION, now(), DEFAULT_TTL_SECS);
        assert_ne!(a, b);
    }

    #[test]
    fn test_action_token_error_debug_clone_copy_eq() {
        let e = ActionTokenError::Expired;
        let cloned = e;
        assert_eq!(e, cloned);
        assert!(format!("{e:?}").contains("Expired"));
    }
}
