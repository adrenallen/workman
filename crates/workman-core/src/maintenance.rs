//! Bounded cleanup of transient records. User documents never expire.
use crate::{Store, StoreResult};
use rusqlite::params;

const DAY_MS: i64 = 86_400_000;
const BATCH: i64 = 1000;

impl Store {
    /// Each pass deletes at most 1,000 rows per category, then allows SQLite to reuse pages.
    /// Old databases retain their existing auto-vacuum mode: no blocking full rewrite on upgrade.
    pub fn maintain_storage(&self, now_ms: i64) -> StoreResult<usize> {
        let transaction = self.connection().unchecked_transaction()?;
        let mut removed = transaction.execute(
            "DELETE FROM notifications WHERE id IN (SELECT id FROM notifications
             WHERE (read_at IS NOT NULL AND read_at < ?1) OR created_at < ?2 LIMIT ?3)",
            params![
                now_ms.saturating_sub(30 * DAY_MS),
                now_ms.saturating_sub(90 * DAY_MS),
                BATCH
            ],
        )?;
        removed += transaction.execute(
            "DELETE FROM timers WHERE id IN (SELECT id FROM timers
             WHERE fired = 1 AND loop = 0 AND fired_at < ?1 LIMIT ?2)",
            params![now_ms.saturating_sub(7 * DAY_MS), BATCH],
        )?;
        removed += transaction.execute(
            "DELETE FROM locks WHERE rowid IN (SELECT rowid FROM locks WHERE acquired_at + ttl <= ?1 LIMIT ?2)",
            params![now_ms, BATCH],
        )?;
        removed += transaction.execute(
            "DELETE FROM actors WHERE id IN (SELECT id FROM actors
             WHERE process_id IS NULL AND last_seen_at < ?1
               AND NOT EXISTS (SELECT 1 FROM locks WHERE owner_actor = actors.id)
               AND NOT EXISTS (SELECT 1 FROM timers WHERE owner_actor = actors.id AND fired = 0)
             LIMIT ?2)",
            params![now_ms.saturating_sub(90 * DAY_MS), BATCH],
        )?;
        transaction.commit()?;
        // Incremental vacuum is a no-op for existing databases in NONE mode. New databases
        // can return up to 256 free pages per pass without a full database rebuild.
        self.connection()
            .execute_batch("PRAGMA incremental_vacuum(256);")?;
        let _: (i64, i64, i64) =
            self.connection()
                .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        Ok(removed)
    }
}
