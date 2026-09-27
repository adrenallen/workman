//! Durable per-owner accounting for completed agent turns.

use std::{collections::BTreeMap, error::Error, fmt};

use rusqlite::{OptionalExtension, params, params_from_iter};
use workman_core::{ProcessId, Store, StoreError, attention::AttentionState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Completion {
    pub id: i64,
    pub process_id: ProcessId,
    pub completed_at_ms: i64,
}

#[derive(Debug)]
pub enum CompletionLedgerError {
    Store(StoreError),
}

impl fmt::Display for CompletionLedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl Error for CompletionLedgerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
        }
    }
}

impl From<StoreError> for CompletionLedgerError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<rusqlite::Error> for CompletionLedgerError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(error))
    }
}

pub(crate) type CompletionLedgerResult<T> = Result<T, CompletionLedgerError>;

/// Small internal API shared by idle timers and child-to-parent notification delivery.
pub(crate) struct CompletionLedger<'a> {
    store: &'a Store,
}

impl<'a> CompletionLedger<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Attribute one successfully queued line submission to the process that sent it.
    pub(crate) fn record_input(
        &self,
        owner_process_id: ProcessId,
        process_id: ProcessId,
        input_at_ms: i64,
    ) -> CompletionLedgerResult<()> {
        self.store.connection().execute(
            "INSERT INTO process_completion_inputs
                (owner_process_id, process_id, last_input_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(owner_process_id, process_id) DO UPDATE SET
                last_input_at = MAX(last_input_at, excluded.last_input_at)",
            params![owner_process_id, process_id, input_at_ms],
        )?;
        Ok(())
    }

    /// Observe one authoritative attention snapshot and record a completed work episode once.
    ///
    /// A completion requires an idle snapshot with no queued prompt plus eager evidence of work
    /// after the submitted input. Evidence is either an adapter-recognized busy state or, for an
    /// adapter without busy detection, non-cosmetic PTY output observed after the recent-input
    /// grace period. Busy-detecting adapters may advance evidence so a real turn end supersedes a
    /// transient mid-turn idle observation. Generic adapters retain their first evidence and
    /// therefore record at most one completion per input.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn observe_process(
        &self,
        process_id: ProcessId,
        state: AttentionState,
        last_input_at: Option<i64>,
        work_evidence_at: Option<i64>,
        has_pending_prompts: bool,
        observed_at_ms: i64,
    ) -> CompletionLedgerResult<Option<Completion>> {
        if has_pending_prompts || !matches!(state, AttentionState::Idle | AttentionState::Waiting) {
            return Ok(None);
        }
        let Some((input_at, work_evidence_at)) = last_input_at.zip(work_evidence_at) else {
            return Ok(None);
        };
        let previous_completion = self
            .store
            .connection()
            .query_row(
                "SELECT last_completed_input_at, last_completed_work_evidence_at
                 FROM process_completion_observations WHERE process_id = ?1",
                [process_id],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        if previous_completion == Some((Some(input_at), Some(work_evidence_at))) {
            return Ok(None);
        }

        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO process_completion_observations
                (process_id, last_completed_input_at, last_completed_work_evidence_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(process_id) DO UPDATE SET
                last_completed_input_at = excluded.last_completed_input_at,
                last_completed_work_evidence_at = excluded.last_completed_work_evidence_at",
            params![process_id, input_at, work_evidence_at],
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO process_completions
                (process_id, input_at, work_evidence_at, completed_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![process_id, input_at, work_evidence_at, observed_at_ms],
        )?;
        let completion = transaction
            .query_row(
                "SELECT id, process_id, completed_at
                 FROM process_completions
                 WHERE process_id = ?1 AND input_at = ?2 AND work_evidence_at = ?3",
                params![process_id, input_at, work_evidence_at],
                completion_from_row,
            )
            .optional()?;
        transaction.commit()?;
        Ok(completion)
    }

    /// Return the newest eligible completion for each watched process.
    ///
    /// Eligibility is owner-local: the completion follows that owner's most recent input and is
    /// newer than the last completion reported to that owner. Owners with no recorded input are
    /// deliberately omitted so idle timers retain their historical fresh-transition behavior.
    pub(crate) fn unreported_completions(
        &self,
        owner_process_id: ProcessId,
        process_ids: &[ProcessId],
    ) -> CompletionLedgerResult<Vec<Completion>> {
        if process_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", process_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT completion.id, completion.process_id, completion.completed_at
             FROM process_completion_inputs AS input
             JOIN process_completions AS completion
               ON completion.process_id = input.process_id
             LEFT JOIN process_completion_reports AS report
               ON report.owner_process_id = input.owner_process_id
              AND report.process_id = input.process_id
             WHERE input.owner_process_id = ?
               AND input.process_id IN ({placeholders})
               AND completion.completed_at > input.last_input_at
               AND completion.id > COALESCE(report.completion_id, 0)
               AND completion.id = (
                   SELECT MAX(candidate.id)
                   FROM process_completions AS candidate
                   WHERE candidate.process_id = input.process_id
                     AND candidate.completed_at > input.last_input_at
                     AND candidate.id > COALESCE(report.completion_id, 0)
               )
             ORDER BY completion.process_id"
        );
        let values = std::iter::once(owner_process_id).chain(process_ids.iter().copied());
        let mut statement = self.store.connection().prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), completion_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub(crate) fn latest_completion(
        &self,
        process_id: ProcessId,
    ) -> CompletionLedgerResult<Option<Completion>> {
        Ok(self
            .store
            .connection()
            .query_row(
                "SELECT id, process_id, completed_at
                 FROM process_completions
                 WHERE process_id = ?1
                 ORDER BY id DESC LIMIT 1",
                [process_id],
                completion_from_row,
            )
            .optional()?)
    }

    /// Mark one completion (and all older ones for that process) as reported to an owner.
    pub(crate) fn mark_completion_reported(
        &self,
        owner_process_id: ProcessId,
        process_id: ProcessId,
        completion_id: i64,
    ) -> CompletionLedgerResult<bool> {
        let belongs_to_process = self.store.connection().query_row(
            "SELECT EXISTS(
                SELECT 1 FROM process_completions WHERE id = ?1 AND process_id = ?2
             )",
            params![completion_id, process_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !belongs_to_process {
            // The watched process (and therefore its completion) may have been
            // deleted after delivery was queued. Reporting is bookkeeping, so
            // a vanished target is an idempotent no-op rather than a failure.
            return Ok(false);
        }
        let changed = self.store.connection().execute(
            "INSERT INTO process_completion_reports
                (owner_process_id, process_id, completion_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(owner_process_id, process_id) DO UPDATE SET
                completion_id = excluded.completion_id
             WHERE excluded.completion_id > process_completion_reports.completion_id",
            params![owner_process_id, process_id, completion_id],
        )?;
        Ok(changed > 0)
    }

    pub(crate) fn mark_completions_reported(
        &self,
        owner_process_id: ProcessId,
        completions: impl IntoIterator<Item = Completion>,
    ) -> CompletionLedgerResult<()> {
        let newest_by_process = completions.into_iter().fold(
            BTreeMap::<ProcessId, Completion>::new(),
            |mut newest, completion| {
                newest
                    .entry(completion.process_id)
                    .and_modify(|current| {
                        if completion.id > current.id {
                            *current = completion;
                        }
                    })
                    .or_insert(completion);
                newest
            },
        );
        for completion in newest_by_process.into_values() {
            self.mark_completion_reported(owner_process_id, completion.process_id, completion.id)?;
        }
        Ok(())
    }
}

fn completion_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Completion> {
    Ok(Completion {
        id: row.get(0)?,
        process_id: row.get(1)?,
        completed_at_ms: row.get(2)?,
    })
}

#[cfg(test)]
mod tests {
    use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};

    use super::*;

    fn put_fixture(store: &Store) {
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/completion-ledger".into(),
                name: "completion-ledger".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 0,
            })
            .unwrap();
        for (id, name) in [(1, "owner-one"), (2, "owner-two"), (3, "worker")] {
            store
                .put_process(&Process {
                    id,
                    project_id: 1,
                    kind: ProcessKind::Agent,
                    name: name.into(),
                    command: None,
                    working_dir: "/tmp/completion-ledger".into(),
                    env: Default::default(),
                    auto_start: false,
                    auto_restart: false,
                    restart_when_changed: Vec::new(),
                    source: ProcessSource::Local,
                    trust_hash: None,
                    status: ProcessStatus::Stopped,
                    pid: None,
                    exit_code: None,
                    exit_signal: None,
                    exited_at: None,
                    agent_tool_id: None,
                    spawned_by_process_id: None,
                    sort_order: 0,
                })
                .unwrap();
        }
    }

    #[test]
    fn owner_reports_are_independent_and_survive_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("workman.sqlite3");
        let completion_id = {
            let store = Store::open(&database).unwrap();
            put_fixture(&store);
            let ledger = CompletionLedger::new(&store);
            ledger.record_input(1, 3, 100).unwrap();
            ledger.record_input(2, 3, 100).unwrap();
            let completion = ledger
                .observe_process(3, AttentionState::Idle, Some(101), Some(105), false, 110)
                .unwrap()
                .unwrap();
            assert_eq!(
                ledger.unreported_completions(1, &[3]).unwrap(),
                vec![completion]
            );
            assert_eq!(
                ledger.unreported_completions(2, &[3]).unwrap(),
                vec![completion]
            );
            assert!(
                ledger
                    .mark_completion_reported(1, 3, completion.id)
                    .unwrap()
            );
            completion.id
        };

        let store = Store::open(&database).unwrap();
        let ledger = CompletionLedger::new(&store);
        assert!(ledger.unreported_completions(1, &[3]).unwrap().is_empty());
        let owner_two = ledger.unreported_completions(2, &[3]).unwrap();
        assert_eq!(owner_two.len(), 1);
        assert_eq!(owner_two[0].id, completion_id);
        assert!(
            ledger
                .mark_completion_reported(2, 3, completion_id)
                .unwrap()
        );
        assert!(ledger.unreported_completions(2, &[3]).unwrap().is_empty());
    }

    #[test]
    fn prompt_without_work_evidence_is_not_a_completion() {
        let store = Store::open_in_memory().unwrap();
        put_fixture(&store);
        let ledger = CompletionLedger::new(&store);
        ledger.record_input(1, 3, 200).unwrap();
        assert!(
            ledger
                .observe_process(3, AttentionState::Idle, Some(201), None, false, 300)
                .unwrap()
                .is_none()
        );
        assert!(ledger.unreported_completions(1, &[3]).unwrap().is_empty());
    }

    #[test]
    fn later_work_evidence_supersedes_an_early_completion_for_the_same_input() {
        let store = Store::open_in_memory().unwrap();
        put_fixture(&store);
        let ledger = CompletionLedger::new(&store);
        ledger.record_input(1, 3, 100).unwrap();

        let early = ledger
            .observe_process(3, AttentionState::Idle, Some(101), Some(110), false, 200)
            .unwrap()
            .unwrap();
        ledger.mark_completion_reported(1, 3, early.id).unwrap();
        assert!(
            ledger
                .observe_process(3, AttentionState::Idle, Some(101), Some(110), false, 250)
                .unwrap()
                .is_none()
        );

        let final_completion = ledger
            .observe_process(3, AttentionState::Idle, Some(101), Some(300), false, 400)
            .unwrap()
            .unwrap();
        assert!(final_completion.id > early.id);
        assert_eq!(
            ledger.unreported_completions(1, &[3]).unwrap(),
            vec![final_completion]
        );
    }

    #[test]
    fn pending_prompt_and_deleted_completion_are_safe_no_ops() {
        let store = Store::open_in_memory().unwrap();
        put_fixture(&store);
        let ledger = CompletionLedger::new(&store);
        assert!(
            ledger
                .observe_process(3, AttentionState::Idle, Some(100), Some(120), true, 200)
                .unwrap()
                .is_none()
        );
        assert!(!ledger.mark_completion_reported(1, 3, 999).unwrap());
    }
}
