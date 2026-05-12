//! Postgres persistence layer.
//!
//! Only **public metadata** lives here — pubkeys, handles, team admin keys
//! and admin signatures. No file content, no team data keys, no sigchain.
//!
//! Uses sqlx runtime queries (not the compile-time `query!` macros) so the
//! binary builds without a live database connection.

use crate::error::{BrokerError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use uuid::Uuid;

pub type DbPool = Pool<Postgres>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub pubkey: String,
    pub handle: Option<String>,
    pub metadata: serde_json::Value,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleRecord {
    pub handle: String,
    pub pubkey: String,
    pub claimed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRecord {
    pub team_id: Uuid,
    pub name: String,
    pub admin_pubkey: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemberRecord {
    pub team_id: Uuid,
    pub member_pubkey: String,
    pub admin_signature: String,
    pub joined_at: DateTime<Utc>,
}

pub async fn connect(database_url: &str) -> Result<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| BrokerError::Internal(format!("migration: {e}")))?;
    Ok(())
}

// --------- Peers ---------

pub async fn upsert_peer(pool: &DbPool, pubkey: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO peers (pubkey, last_seen)
        VALUES ($1, NOW())
        ON CONFLICT (pubkey)
        DO UPDATE SET last_seen = NOW()
        "#,
    )
    .bind(pubkey)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_peer(pool: &DbPool, pubkey: &str) -> Result<Option<PeerRecord>> {
    let rec: Option<PeerRecord> = sqlx::query_as::<_, PeerRow>(
        r#"
        SELECT pubkey, handle, metadata, first_seen, last_seen
        FROM peers WHERE pubkey = $1
        "#,
    )
    .bind(pubkey)
    .fetch_optional(pool)
    .await?
    .map(PeerRow::into_record);
    Ok(rec)
}

#[derive(sqlx::FromRow)]
struct PeerRow {
    pubkey: String,
    handle: Option<String>,
    metadata: serde_json::Value,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
}

impl PeerRow {
    fn into_record(self) -> PeerRecord {
        PeerRecord {
            pubkey: self.pubkey,
            handle: self.handle,
            metadata: self.metadata,
            first_seen: self.first_seen,
            last_seen: self.last_seen,
        }
    }
}

// --------- Handles ---------

/// Atomically claim a handle for a pubkey. Fails with
/// `BrokerError::HandleTaken` if another pubkey already owns it, or
/// `BrokerError::HandleAlreadyOwned` if the pubkey already owns a
/// (different) handle.
pub async fn claim_handle(pool: &DbPool, pubkey: &str, handle: &str) -> Result<()> {
    let mut tx = pool.begin().await?;

    // Has this pubkey already claimed a (different) handle? Free-tier rule:
    // 1 handle per pubkey.
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT handle FROM handles WHERE pubkey = $1",
    )
    .bind(pubkey)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((existing_handle,)) = existing {
        if existing_handle == handle {
            tx.rollback().await?;
            return Ok(()); // idempotent
        }
        tx.rollback().await?;
        return Err(BrokerError::HandleAlreadyOwned);
    }

    // Is the handle free?
    let taken: Option<(String,)> =
        sqlx::query_as("SELECT pubkey FROM handles WHERE handle = $1")
            .bind(handle)
            .fetch_optional(&mut *tx)
            .await?;
    if taken.is_some() {
        tx.rollback().await?;
        return Err(BrokerError::HandleTaken);
    }

    sqlx::query("INSERT INTO handles (handle, pubkey) VALUES ($1, $2)")
        .bind(handle)
        .bind(pubkey)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE peers SET handle = $1 WHERE pubkey = $2")
        .bind(handle)
        .bind(pubkey)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn resolve_handle(pool: &DbPool, handle: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT pubkey FROM handles WHERE handle = $1")
            .bind(handle)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(pk,)| pk))
}

// --------- Teams ---------

pub async fn upsert_team(
    pool: &DbPool,
    team_id: Option<Uuid>,
    name: &str,
    admin_pubkey: &str,
) -> Result<Uuid> {
    if let Some(id) = team_id {
        sqlx::query(
            r#"
            INSERT INTO teams (team_id, name, admin_pubkey)
            VALUES ($1, $2, $3)
            ON CONFLICT (team_id) DO UPDATE
              SET name = EXCLUDED.name
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(admin_pubkey)
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO teams (name, admin_pubkey) VALUES ($1, $2) RETURNING team_id",
        )
        .bind(name)
        .bind(admin_pubkey)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}

pub async fn add_team_member(
    pool: &DbPool,
    team_id: Uuid,
    member_pubkey: &str,
    admin_signature: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO team_members (team_id, member_pubkey, admin_signature)
        VALUES ($1, $2, $3)
        ON CONFLICT (team_id, member_pubkey) DO UPDATE
          SET admin_signature = EXCLUDED.admin_signature
        "#,
    )
    .bind(team_id)
    .bind(member_pubkey)
    .bind(admin_signature)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn team_members(pool: &DbPool, team_id: Uuid) -> Result<Vec<TeamMemberRecord>> {
    let rows = sqlx::query_as::<_, TeamMemberRow>(
        r#"
        SELECT team_id, member_pubkey, admin_signature, joined_at
        FROM team_members WHERE team_id = $1
        ORDER BY joined_at ASC
        "#,
    )
    .bind(team_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TeamMemberRow::into_record).collect())
}

pub async fn get_team(pool: &DbPool, team_id: Uuid) -> Result<Option<TeamRecord>> {
    let row: Option<TeamRow> = sqlx::query_as::<_, TeamRow>(
        "SELECT team_id, name, admin_pubkey, created_at FROM teams WHERE team_id = $1",
    )
    .bind(team_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(TeamRow::into_record))
}

#[derive(sqlx::FromRow)]
struct TeamRow {
    team_id: Uuid,
    name: String,
    admin_pubkey: String,
    created_at: DateTime<Utc>,
}
impl TeamRow {
    fn into_record(self) -> TeamRecord {
        TeamRecord {
            team_id: self.team_id,
            name: self.name,
            admin_pubkey: self.admin_pubkey,
            created_at: self.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct TeamMemberRow {
    team_id: Uuid,
    member_pubkey: String,
    admin_signature: String,
    joined_at: DateTime<Utc>,
}
impl TeamMemberRow {
    fn into_record(self) -> TeamMemberRecord {
        TeamMemberRecord {
            team_id: self.team_id,
            member_pubkey: self.member_pubkey,
            admin_signature: self.admin_signature,
            joined_at: self.joined_at,
        }
    }
}
