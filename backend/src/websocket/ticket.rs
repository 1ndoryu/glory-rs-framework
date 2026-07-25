use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::errors::AppError;

type HmacSha256 = Hmac<Sha256>;

const DEFAULT_TTL_SECS: i64 = 60;
const MAX_TTL_SECS: i64 = 300;

/* [174A-69] Tickets HMAC de vida corta para handshakes websocket.
 * Son stateless a propósito: 174A-70/73 decidirán si hace falta revocación o
 * almacenamiento compartido. Por ahora el contrato mínimo es user_id + exp + nonce. */

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketTicketClaims {
    pub user_id: i32,
    pub exp: i64,
    pub nonce: Uuid,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn normalize_ttl(ttl_secs: i64) -> i64 {
    if ttl_secs <= 0 {
        DEFAULT_TTL_SECS
    } else {
        ttl_secs.min(MAX_TTL_SECS)
    }
}

fn sign(payload: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC acepta cualquier longitud de key");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[must_use]
pub fn generate(user_id: i32, ttl_secs: i64, secret: &str) -> String {
    let exp = now_unix().saturating_add(normalize_ttl(ttl_secs));
    let nonce = Uuid::new_v4();
    let payload = format!("{user_id}:{exp}:{nonce}");
    let signature = sign(&payload, secret);
    URL_SAFE_NO_PAD.encode(format!("{payload}:{signature}").as_bytes())
}

pub fn verify(token: &str, secret: &str) -> Result<WebSocketTicketClaims, AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|_| AppError::Unauthorized)?;
    let decoded = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;
    let parts: Vec<&str> = decoded.split(':').collect();
    if parts.len() != 4 {
        return Err(AppError::Unauthorized);
    }

    let user_id = parts[0]
        .parse::<i32>()
        .map_err(|_| AppError::Unauthorized)?;
    let exp = parts[1]
        .parse::<i64>()
        .map_err(|_| AppError::Unauthorized)?;
    let nonce = parts[2]
        .parse::<Uuid>()
        .map_err(|_| AppError::Unauthorized)?;
    if now_unix() > exp {
        return Err(AppError::Forbidden("ticket websocket expirado".into()));
    }

    let payload = format!("{user_id}:{exp}:{nonce}");
    let expected_signature = sign(&payload, secret);
    if !constant_time_eq(expected_signature.as_bytes(), parts[3].as_bytes()) {
        return Err(AppError::Unauthorized);
    }

    Ok(WebSocketTicketClaims {
        user_id,
        exp,
        nonce,
    })
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{generate, sign, verify, DEFAULT_TTL_SECS, MAX_TTL_SECS};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }

    #[test]
    fn round_trip_ticket() {
        let token = generate(42, 90, "secret");
        let claims = verify(&token, "secret").expect("claims");
        assert_eq!(claims.user_id, 42);
        assert!(claims.exp >= now_unix() + 80);
    }

    #[test]
    fn invalid_secret_rejected() {
        let token = generate(7, 30, "secret-a");
        assert!(verify(&token, "secret-b").is_err());
    }

    #[test]
    fn expired_ticket_rejected() {
        let nonce = Uuid::new_v4();
        let payload = format!("1:{}:{nonce}", now_unix() - 5);
        let raw = format!("{payload}:{}", sign(&payload, "secret"));
        let token = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        assert!(verify(&token, "secret").is_err());
    }

    #[test]
    fn ttl_is_bounded() {
        let fallback = generate(1, 0, "secret");
        let fallback_claims = verify(&fallback, "secret").expect("fallback claims");
        assert!(fallback_claims.exp <= now_unix() + DEFAULT_TTL_SECS + 1);

        let capped = generate(1, MAX_TTL_SECS * 10, "secret");
        let capped_claims = verify(&capped, "secret").expect("capped claims");
        assert!(capped_claims.exp <= now_unix() + MAX_TTL_SECS + 1);
    }
}
