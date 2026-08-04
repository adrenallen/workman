//! Project-scoped advisory leases for cross-session coordination.

use std::{error::Error, fmt};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{ProjectId, ProjectLock, Store, StoreError};

pub const MAX_LOCK_KEY_BYTES: usize = 128;
pub const MAX_LOCK_LEASE_TTL_MS: i64 = 86_400_000;

/// A live project-scoped lease with its computed deadline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseView {
    pub project_id: ProjectId,
    pub lock_key: String,
    pub owner_actor_id: String,
    pub acquired_at: i64,
    pub expires_at: i64,
}

impl From<ProjectLock> for LeaseView {
    fn from(lock: ProjectLock) -> Self {
        Self {
            project_id: lock.project_id,
            lock_key: lock.key,
            owner_actor_id: lock.owner_actor,
            acquired_at: lock.acquired_at,
            expires_at: lock.acquired_at.saturating_add(lock.ttl_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockServiceError {
    Store(String),
    ProjectNotFound(ProjectId),
    InvalidKey(String),
    InvalidActor,
    InvalidLeaseTtl,
    Held {
        lock_key: String,
        owner_actor_id: String,
        expires_at: i64,
    },
    NotOwned {
        lock_key: String,
        owner_actor_id: String,
    },
}

impl LockServiceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "store_error",
            Self::ProjectNotFound(_) => "project_not_found",
            Self::InvalidKey(_) => "invalid_lock_key",
            Self::InvalidActor => "invalid_lock_actor",
            Self::InvalidLeaseTtl => "invalid_lease_ttl",
            Self::Held { .. } => "lock_held",
            Self::NotOwned { .. } => "lock_not_owned",
        }
    }
}

impl fmt::Display for LockServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(message) => formatter.write_str(message),
            Self::ProjectNotFound(project_id) => {
                write!(formatter, "project {project_id} was not found")
            }
            Self::InvalidKey(message) => formatter.write_str(message),
            Self::InvalidActor => formatter.write_str("lock actor must not be empty"),
            Self::InvalidLeaseTtl => write!(
                formatter,
                "lease TTL must be between 1 and {} milliseconds",
                MAX_LOCK_LEASE_TTL_MS
            ),
            Self::Held {
                lock_key,
                owner_actor_id,
                expires_at,
            } => write!(
                formatter,
                "lock {lock_key:?} is held by {owner_actor_id} until {expires_at}"
            ),
            Self::NotOwned {
                lock_key,
                owner_actor_id,
            } => write!(
                formatter,
                "lock {lock_key:?} is owned by {owner_actor_id}, not this actor"
            ),
        }
    }
}

impl Error for LockServiceError {}

impl From<StoreError> for LockServiceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<rusqlite::Error> for LockServiceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.to_string())
    }
}

pub type LockServiceResult<T> = Result<T, LockServiceError>;

pub struct LockService<'store> {
    store: &'store Store,
}

impl<'store> LockService<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// Try to acquire or renew a lease without waiting for another owner.
    pub fn acquire(
        &self,
        project_id: ProjectId,
        lock_key: &str,
        actor_id: &str,
        lease_ttl_ms: i64,
        now_ms: i64,
    ) -> LockServiceResult<LeaseView> {
        self.require_project(project_id)?;
        validate_lock_key(lock_key)?;
        validate_actor(actor_id)?;
        if !(1..=MAX_LOCK_LEASE_TTL_MS).contains(&lease_ttl_ms) {
            return Err(LockServiceError::InvalidLeaseTtl);
        }

        let changed = self.store.connection().execute(
            "INSERT INTO locks (project_id, key, owner_actor, acquired_at, ttl)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id, key) DO UPDATE SET
                owner_actor = excluded.owner_actor,
                acquired_at = excluded.acquired_at,
                ttl = excluded.ttl
             WHERE locks.owner_actor = excluded.owner_actor
                OR locks.acquired_at + locks.ttl <= excluded.acquired_at",
            params![project_id, lock_key, actor_id, now_ms, lease_ttl_ms],
        )?;
        if changed == 0 {
            let lease = self
                .active_lease(project_id, lock_key, now_ms)?
                .ok_or_else(|| LockServiceError::Store("lock changed concurrently".into()))?;
            return Err(LockServiceError::Held {
                lock_key: lock_key.into(),
                owner_actor_id: lease.owner_actor_id,
                expires_at: lease.expires_at,
            });
        }
        self.active_lease(project_id, lock_key, now_ms)?
            .ok_or_else(|| LockServiceError::Store("acquired lock could not be read back".into()))
    }

    /// Release a live lease only when `actor_id` owns it.
    pub fn release(
        &self,
        project_id: ProjectId,
        lock_key: &str,
        actor_id: &str,
        now_ms: i64,
    ) -> LockServiceResult<bool> {
        self.require_project(project_id)?;
        validate_lock_key(lock_key)?;
        validate_actor(actor_id)?;
        let changed = self.store.connection().execute(
            "DELETE FROM locks
             WHERE project_id = ?1 AND key = ?2 AND owner_actor = ?3
               AND acquired_at + ttl > ?4",
            params![project_id, lock_key, actor_id, now_ms],
        )?;
        if changed > 0 {
            return Ok(true);
        }
        match self.active_lease(project_id, lock_key, now_ms)? {
            Some(lease) => Err(LockServiceError::NotOwned {
                lock_key: lock_key.into(),
                owner_actor_id: lease.owner_actor_id,
            }),
            None => Ok(false),
        }
    }

    /// Return the live lease, treating expired rows as absent.
    pub fn status(
        &self,
        project_id: ProjectId,
        lock_key: &str,
        now_ms: i64,
    ) -> LockServiceResult<Option<LeaseView>> {
        self.require_project(project_id)?;
        validate_lock_key(lock_key)?;
        self.active_lease(project_id, lock_key, now_ms)
    }

    fn require_project(&self, project_id: ProjectId) -> LockServiceResult<()> {
        if self.store.get_project(project_id)?.is_none() {
            return Err(LockServiceError::ProjectNotFound(project_id));
        }
        Ok(())
    }

    fn active_lease(
        &self,
        project_id: ProjectId,
        lock_key: &str,
        now_ms: i64,
    ) -> LockServiceResult<Option<LeaseView>> {
        self.store.connection().execute(
            "DELETE FROM locks
             WHERE project_id = ?1 AND key = ?2 AND acquired_at + ttl <= ?3",
            params![project_id, lock_key, now_ms],
        )?;
        Ok(self
            .store
            .get_project_lock(project_id, lock_key)?
            .map(LeaseView::from))
    }
}

fn validate_actor(actor_id: &str) -> LockServiceResult<()> {
    if actor_id.trim().is_empty() {
        return Err(LockServiceError::InvalidActor);
    }
    Ok(())
}

fn validate_lock_key(lock_key: &str) -> LockServiceResult<()> {
    if lock_key.is_empty() || lock_key.len() > MAX_LOCK_KEY_BYTES {
        return Err(LockServiceError::InvalidKey(format!(
            "lock_key must contain between 1 and {MAX_LOCK_KEY_BYTES} bytes"
        )));
    }
    let mut bytes = lock_key.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(LockServiceError::InvalidKey(
            "lock_key must start with a lowercase letter or digit".into(),
        ));
    }
    if !bytes.all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
    }) {
        return Err(LockServiceError::InvalidKey(
            "lock_key may contain only lowercase letters, digits, '.', '_', ':', '/', and '-'"
                .into(),
        ));
    }
    Ok(())
}
