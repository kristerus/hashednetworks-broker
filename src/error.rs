use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("invalid public key: {0}")]
    InvalidPubKey(String),
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("signature verification failed")]
    SignatureVerifyFailed,
    #[error("message too old (drift {drift_secs}s > 60s)")]
    StaleMessage { drift_secs: i64 },
    #[error("message in the future (drift {drift_secs}s > 60s)")]
    FutureMessage { drift_secs: i64 },
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("handle already claimed")]
    HandleTaken,
    #[error("peer already has a handle claimed")]
    HandleAlreadyOwned,
    #[error("peer not found")]
    PeerNotFound,
    #[error("peer offline")]
    PeerOffline,
    #[error("payload too large ({size} > {max})")]
    PayloadTooLarge { size: usize, max: usize },
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("relay quota exceeded")]
    RelayQuotaExceeded,
    #[error("relay peer did not consent")]
    RelayNotConsented,
    #[error("relay session not found")]
    RelaySessionNotFound,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal: {0}")]
    Internal(String),
}

impl BrokerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPubKey(_) => "invalid_pubkey",
            Self::InvalidSignature(_) => "invalid_signature",
            Self::SignatureVerifyFailed => "signature_verify_failed",
            Self::StaleMessage { .. } => "stale_message",
            Self::FutureMessage { .. } => "future_message",
            Self::Malformed(_) => "malformed",
            Self::HandleTaken => "handle_taken",
            Self::HandleAlreadyOwned => "handle_already_owned",
            Self::PeerNotFound => "peer_not_found",
            Self::PeerOffline => "peer_offline",
            Self::PayloadTooLarge { .. } => "payload_too_large",
            Self::RateLimited => "rate_limited",
            Self::RelayQuotaExceeded => "relay_quota_exceeded",
            Self::RelayNotConsented => "relay_not_consented",
            Self::RelaySessionNotFound => "relay_session_not_found",
            Self::Database(_) => "database_error",
            Self::Io(_) => "io_error",
            Self::Internal(_) => "internal_error",
        }
    }
}

pub type Result<T> = std::result::Result<T, BrokerError>;
