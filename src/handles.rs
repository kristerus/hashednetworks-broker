//! Handle claiming.
//!
//! Wraps the DB layer's atomic-claim primitive and applies the free-tier
//! rule (1 handle per pubkey). Handles are validated: lowercase
//! alphanumerics, `.`, `-`, `_`, and an optional `@org` suffix; length 2..=64.

use crate::db::{self, DbPool};
use crate::error::{BrokerError, Result};

pub fn validate_handle(handle: &str) -> Result<()> {
    let len = handle.len();
    if !(2..=64).contains(&len) {
        return Err(BrokerError::Malformed(format!(
            "handle length {len} not in [2, 64]"
        )));
    }
    let mut at_count = 0;
    for ch in handle.chars() {
        match ch {
            'a'..='z' | '0'..='9' | '.' | '-' | '_' => {}
            '@' => {
                at_count += 1;
                if at_count > 1 {
                    return Err(BrokerError::Malformed("handle has >1 '@'".into()));
                }
            }
            other => {
                return Err(BrokerError::Malformed(format!(
                    "handle char {other:?} not allowed (lowercase alnum, .-_ or single @)"
                )));
            }
        }
    }
    Ok(())
}

pub async fn claim(pool: &DbPool, pubkey: &str, handle: &str) -> Result<()> {
    validate_handle(handle)?;
    db::upsert_peer(pool, pubkey).await?;
    db::claim_handle(pool, pubkey, handle).await
}

pub async fn resolve(pool: &DbPool, handle: &str) -> Result<Option<String>> {
    db::resolve_handle(pool, handle).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_handles_accept() {
        for h in ["alice", "ali_ce", "alice.bob", "alice-bob", "alice@org", "a1"] {
            validate_handle(h).expect(h);
        }
    }

    #[test]
    fn invalid_handles_reject() {
        for h in ["a", "Alice", "alice org", "al ice", "al!ce", "a@b@c", "x".repeat(65).as_str()] {
            assert!(validate_handle(h).is_err(), "should reject {h:?}");
        }
    }
}
