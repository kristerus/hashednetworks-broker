//! Team membership tracking.
//!
//! Membership rule: a team has one admin pubkey. To add a member, the
//! admin signs `team_id_bytes || member_pubkey_bytes` and includes that
//! signature in a `team_announce` message. The broker:
//!   1. Verifies the message-level signature (the announcing peer holds the
//!      key they claim).
//!   2. Verifies the admin signature over the membership tuple under the
//!      claimed admin pubkey.
//!   3. Persists.

use crate::auth;
use crate::db::{self, DbPool, TeamMemberRecord};
use crate::error::{BrokerError, Result};
use uuid::Uuid;

/// Bytes the admin signs: 16 bytes of team UUID followed by 32 bytes of
/// member pubkey (raw, hex-decoded).
fn admin_signed_bytes(team_id: &Uuid, member_pubkey_hex: &str) -> Result<Vec<u8>> {
    let member_bytes = hex::decode(member_pubkey_hex.strip_prefix("ed25519:").unwrap_or(member_pubkey_hex))
        .map_err(|e| BrokerError::InvalidPubKey(e.to_string()))?;
    if member_bytes.len() != 32 {
        return Err(BrokerError::InvalidPubKey("member pubkey must be 32 bytes".into()));
    }
    let mut bytes = Vec::with_capacity(48);
    bytes.extend_from_slice(team_id.as_bytes());
    bytes.extend_from_slice(&member_bytes);
    Ok(bytes)
}

pub async fn announce(
    pool: &DbPool,
    announcing_pubkey: &str,
    team_id: Option<Uuid>,
    team_name: &str,
    admin_pubkey: &str,
    admin_signature: &str,
) -> Result<Uuid> {
    // Upsert team (creating one if no id supplied).
    let id = db::upsert_team(pool, team_id, team_name, admin_pubkey).await?;

    // Verify the admin signed (team_id || announcing_pubkey).
    let signed = admin_signed_bytes(&id, announcing_pubkey)?;
    auth::verify_detached(admin_pubkey, admin_signature, &signed)?;

    db::upsert_peer(pool, announcing_pubkey).await?;
    db::add_team_member(pool, id, announcing_pubkey, admin_signature).await?;
    Ok(id)
}

pub async fn members(pool: &DbPool, team_id: Uuid) -> Result<Vec<TeamMemberRecord>> {
    db::team_members(pool, team_id).await
}

pub fn parse_team_id(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| BrokerError::Malformed(format!("team_id: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_signed_bytes_layout() {
        let team_id = Uuid::nil();
        let pk = hex::encode([1u8; 32]);
        let bytes = admin_signed_bytes(&team_id, &pk).unwrap();
        assert_eq!(bytes.len(), 16 + 32);
        assert_eq!(&bytes[..16], team_id.as_bytes());
        assert!(bytes[16..].iter().all(|&b| b == 1));
    }
}
