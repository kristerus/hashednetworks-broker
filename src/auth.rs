//! Signature verification + replay-window enforcement.
//!
//! Every signed message includes a `pubkey`, `timestamp` (unix seconds),
//! and `signature`. The signature is over the canonical bytes (see
//! [`crate::protocol::canonical_bytes`]). We reject anything older than
//! [`MAX_DRIFT_SECS`] (or as much in the future).

use crate::error::{BrokerError, Result};
use crate::protocol::{canonical_bytes, ClientMessage};
use chrono::Utc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Replay-window, both backward and forward (60 s per spec).
pub const MAX_DRIFT_SECS: i64 = 60;

/// Parse a hex-encoded Ed25519 public key. Accepts an optional `ed25519:`
/// prefix.
pub fn parse_pubkey(s: &str) -> Result<VerifyingKey> {
    let hex_str = s.strip_prefix("ed25519:").unwrap_or(s);
    let bytes = hex::decode(hex_str).map_err(|e| BrokerError::InvalidPubKey(e.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| BrokerError::InvalidPubKey("expected 32 bytes".to_string()))?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| BrokerError::InvalidPubKey(e.to_string()))
}

/// Parse a hex-encoded Ed25519 signature.
pub fn parse_signature(s: &str) -> Result<Signature> {
    let bytes = hex::decode(s).map_err(|e| BrokerError::InvalidSignature(e.to_string()))?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| BrokerError::InvalidSignature("expected 64 bytes".to_string()))?;
    Ok(Signature::from_bytes(&bytes))
}

/// Verify a [`ClientMessage`]:
/// 1. Pubkey parses.
/// 2. Timestamp is within [`MAX_DRIFT_SECS`] of `now`.
/// 3. Signature verifies over canonical bytes with the claimed pubkey.
///
/// Returns the parsed pubkey on success.
pub fn verify_signed_message(msg: &ClientMessage) -> Result<VerifyingKey> {
    let pk = parse_pubkey(msg.pubkey())?;
    check_timestamp(msg.timestamp())?;
    let sig = parse_signature(msg.signature())?;
    let bytes = canonical_bytes(msg);
    pk.verify(&bytes, &sig)
        .map_err(|_| BrokerError::SignatureVerifyFailed)?;
    Ok(pk)
}

fn check_timestamp(ts: i64) -> Result<()> {
    let now = Utc::now().timestamp();
    let drift = now - ts;
    if drift > MAX_DRIFT_SECS {
        return Err(BrokerError::StaleMessage { drift_secs: drift });
    }
    if drift < -MAX_DRIFT_SECS {
        return Err(BrokerError::FutureMessage { drift_secs: -drift });
    }
    Ok(())
}

/// Verify an arbitrary bytes payload signature by a given peer pubkey.
/// Used for team admin signatures: the admin signs (member_pubkey ||
/// team_id_bytes) and the broker checks it under the admin pubkey.
pub fn verify_detached(pubkey_hex: &str, sig_hex: &str, message: &[u8]) -> Result<()> {
    let pk = parse_pubkey(pubkey_hex)?;
    let sig = parse_signature(sig_hex)?;
    pk.verify(message, &sig)
        .map_err(|_| BrokerError::SignatureVerifyFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClientMessage, PeerAddresses};
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn sign_register(sk: &SigningKey, ts: i64) -> ClientMessage {
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let mut msg = ClientMessage::Register {
            pubkey: pubkey_hex.clone(),
            handle: Some("alice".into()),
            addresses: PeerAddresses {
                lan: vec!["192.168.1.42:51820".into()],
                public: None,
            },
            kind: None,
            timestamp: ts,
            signature: String::new(),
            request_id: Some("req-1".into()),
        };
        let bytes = canonical_bytes(&msg);
        let sig = sk.sign(&bytes);
        if let ClientMessage::Register { signature, .. } = &mut msg {
            *signature = hex::encode(sig.to_bytes());
        }
        msg
    }

    #[test]
    fn valid_register_verifies() {
        let sk = SigningKey::generate(&mut OsRng);
        let msg = sign_register(&sk, Utc::now().timestamp());
        verify_signed_message(&msg).expect("must verify");
    }

    #[test]
    fn tampered_register_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut msg = sign_register(&sk, Utc::now().timestamp());
        if let ClientMessage::Register { handle, .. } = &mut msg {
            *handle = Some("bob".into()); // tamper after signing
        }
        let err = verify_signed_message(&msg).unwrap_err();
        assert!(matches!(err, BrokerError::SignatureVerifyFailed));
    }

    #[test]
    fn stale_message_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let msg = sign_register(&sk, Utc::now().timestamp() - 120);
        let err = verify_signed_message(&msg).unwrap_err();
        assert!(matches!(err, BrokerError::StaleMessage { .. }));
    }

    #[test]
    fn future_message_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let msg = sign_register(&sk, Utc::now().timestamp() + 120);
        let err = verify_signed_message(&msg).unwrap_err();
        assert!(matches!(err, BrokerError::FutureMessage { .. }));
    }
}
