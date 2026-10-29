//! Keeping the content database from growing forever (docs/protocol.md, section 5.4).

use std::collections::{HashMap, HashSet};

use rusqlite::params;

use super::ContentDb;
use crate::process::ProcessState;
use crate::Result;

/// What to do with a `RunningClient` row after probing its process.
///
/// Only a process that is definitely gone may be dropped: a failed probe (`Unknown`, e.g. access
/// denied or a protected process) must keep the row, otherwise cull can delete a version out from
/// under a running client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Keep,
    Drop,
}

pub fn running_client_action(state: ProcessState) -> RowAction {
    match state {
        ProcessState::Gone => RowAction::Drop,
        ProcessState::Alive | ProcessState::Unknown => RowAction::Keep,
    }
}

/// A foreign key violation on `DELETE FROM ContentVersion` means a running client still uses the
/// version, not that the cull failed.
fn is_constraint_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

#[derive(Debug, Clone, Copy)]
pub struct CullPolicy {
    /// Versions kept in total.
    pub max_versions: usize,
    /// Versions kept per fork id.
    pub max_versions_per_fork: usize,
    /// Interrupted downloads older than this are dropped.
    pub interrupted_keep_hours: u32,
}

impl Default for CullPolicy {
    /// The official launcher's defaults.
    fn default() -> Self {
        Self {
            max_versions: 15,
            max_versions_per_fork: 3,
            interrupted_keep_hours: 7 * 24,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CullReport {
    pub versions_removed: usize,
    /// Old versions that would have been removed but a running client uses them.
    pub versions_in_use: usize,
    pub interrupted_removed: usize,
    pub blobs_removed: usize,
}

/// An interrupted download is never dropped before this many minutes without a heartbeat, even by
/// "clear everything" (`interrupted_keep_hours: 0`). Otherwise a second launcher that is downloading
/// right now would lose its `InterruptedDownload` row and, with it, the blobs it already fetched
/// (and could even hit a foreign key error when a link points at the removed parent).
pub const INTERRUPTED_MIN_GRACE_MINUTES: u32 = 10;

impl ContentDb {
    /// Version ids used by clients that are still running. `state` reports whether a `RunningClient`
    /// row still describes a live process; rows of processes that are definitely gone are deleted,
    /// rows whose process could not be checked are kept (see [`running_client_action`]).
    /// (The Loader writes these rows itself when the client starts and removes them on exit.)
    ///
    /// Reading and deleting `RunningClient` rows happens in one transaction.
    pub fn running_client_versions_with(
        &self,
        state: impl Fn(u32, &str) -> ProcessState,
    ) -> Result<Vec<i64>> {
        self.transaction(|db| {
            let rows: Vec<(i64, String, i64)> = db
                .conn
                .prepare("SELECT ProcessId, MainModule, UsedVersion FROM RunningClient")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<_, _>>()?;

            let mut used = Vec::new();
            for (pid, module, version) in rows {
                let probe = u32::try_from(pid).map_or(ProcessState::Gone, |p| state(p, &module));
                match running_client_action(probe) {
                    RowAction::Keep => used.push(version),
                    RowAction::Drop => {
                        db.conn
                            .execute("DELETE FROM RunningClient WHERE ProcessId = ?1", [pid])?;
                    }
                }
            }
            Ok(used)
        })
    }

    /// [`ContentDb::running_client_versions_with`] for callers that only have a boolean check
    /// (`true` = alive). Prefer the three-state version: this one treats "could not check" as gone.
    pub fn running_client_versions(
        &self,
        is_alive: impl Fn(u32, &str) -> bool,
    ) -> Result<Vec<i64>> {
        self.running_client_versions_with(|pid, module| {
            if is_alive(pid, module) {
                ProcessState::Alive
            } else {
                ProcessState::Gone
            }
        })
    }

    /// Removes old versions, old interrupted downloads and blobs nobody refers to any more.
    ///
    /// The newest versions are kept, at most `max_versions` in total and `max_versions_per_fork` per
    /// fork. A version in `in_use`, or one a `RunningClient` row points at, is never removed. Runs in
    /// one transaction.
    pub fn cull(&self, policy: CullPolicy, in_use: &[i64]) -> Result<CullReport> {
        self.transaction(|db| {
            let mut report = CullReport::default();

            // Rows that survived the liveness check above, plus any a client inserted meanwhile:
            // either way a live client uses the version, and the foreign key would refuse to
            // delete it anyway.
            let mut protected: HashSet<i64> = in_use.iter().copied().collect();
            {
                let mut stmt = db
                    .conn
                    .prepare("SELECT DISTINCT UsedVersion FROM RunningClient")?;
                let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
                for row in rows {
                    protected.insert(row?);
                }
            }

            let versions: Vec<(i64, Option<String>)> = db
                .conn
                .prepare("SELECT Id, ForkId FROM ContentVersion ORDER BY LastUsed DESC, Id DESC")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;

            let mut per_fork: HashMap<Option<String>, usize> = HashMap::new();
            let mut total = 0;
            for (id, fork) in versions {
                let count = per_fork.entry(fork).or_insert(0);
                if *count < policy.max_versions_per_fork && total < policy.max_versions {
                    *count += 1;
                    total += 1;
                } else if protected.contains(&id) {
                    report.versions_in_use += 1;
                } else {
                    // A row added by a client after `in_use` was computed makes the delete fail
                    // with a foreign key error: that means "in use", not "cull failed".
                    match db
                        .conn
                        .execute("DELETE FROM ContentVersion WHERE Id = ?1", [id])
                    {
                        Ok(_) => report.versions_removed += 1,
                        Err(e) if is_constraint_violation(&e) => report.versions_in_use += 1,
                        Err(e) => return Err(e.into()),
                    }
                }
            }

            // `Added` is refreshed on every stored batch (`touch_interrupted_download`), so this is
            // "no activity for N minutes", not "started N minutes ago". The minimum grace keeps a
            // download that another process started a moment ago safe even at keep_hours = 0.
            let keep_minutes = (i64::from(policy.interrupted_keep_hours) * 60)
                .max(i64::from(INTERRUPTED_MIN_GRACE_MINUTES));
            report.interrupted_removed = db.conn.execute(
                "DELETE FROM InterruptedDownload WHERE Added < datetime('now', ?1)",
                params![format!("-{keep_minutes} minutes")],
            )?;

            if report.versions_removed > 0 || report.interrupted_removed > 0 {
                report.blobs_removed = db.conn.execute(
                    "DELETE FROM Content
                     WHERE Id NOT IN (SELECT ContentId FROM ContentManifest)
                       AND Id NOT IN (SELECT ContentId FROM InterruptedDownloadContent)",
                    [],
                )?;
            }
            Ok(report)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::blake2b256;

    /// A version with one unique file. `age` orders LastUsed: bigger is older.
    fn version(db: &ContentDb, fork: &str, name: &str, age: i64) -> i64 {
        let v = db.create_version(fork, name).unwrap();
        let data = format!("file of {fork}/{name}");
        let (cid, _) = db
            .store_blob(&blake2b256(data.as_bytes()), data.as_bytes())
            .unwrap();
        db.add_manifest_entry(v, "file.txt", cid).unwrap();
        db.connection()
            .execute(
                "UPDATE ContentVersion SET LastUsed = datetime('2026-01-01', ?1) WHERE Id = ?2",
                params![format!("-{age} days"), v],
            )
            .unwrap();
        v
    }

    fn ids(db: &ContentDb) -> Vec<i64> {
        db.connection()
            .prepare("SELECT Id FROM ContentVersion ORDER BY Id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    fn blob_count(db: &ContentDb) -> i64 {
        db.connection()
            .query_row("SELECT COUNT(*) FROM Content", [], |r| r.get(0))
            .unwrap()
    }

    fn policy(total: usize, per_fork: usize) -> CullPolicy {
        CullPolicy {
            max_versions: total,
            max_versions_per_fork: per_fork,
            interrupted_keep_hours: 24,
        }
    }

    #[test]
    fn keeps_the_newest_per_fork() {
        let db = ContentDb::open_in_memory().unwrap();
        let a: Vec<i64> = (0..5)
            .map(|i| version(&db, "a", &format!("v{i}"), i))
            .collect(); // v0 newest
        let b = version(&db, "b", "v0", 10);

        let report = db.cull(policy(15, 3), &[]).unwrap();
        assert_eq!(report.versions_removed, 2);
        assert_eq!(ids(&db), [a[0], a[1], a[2], b]);
        assert_eq!(
            report.blobs_removed, 2,
            "the removed versions' files go too"
        );
        assert_eq!(blob_count(&db), 4);
    }

    #[test]
    fn total_limit_removes_the_oldest_overall() {
        let db = ContentDb::open_in_memory().unwrap();
        let v1 = version(&db, "a", "1", 1);
        let v2 = version(&db, "b", "2", 2);
        let v3 = version(&db, "c", "3", 3);
        let v4 = version(&db, "d", "4", 4);
        let report = db.cull(policy(2, 3), &[]).unwrap();
        assert_eq!(report.versions_removed, 2);
        assert_eq!(ids(&db), [v1, v2]);
        let _ = (v3, v4);
    }

    #[test]
    fn versions_in_use_survive() {
        let db = ContentDb::open_in_memory().unwrap();
        let newest = version(&db, "a", "new", 0);
        let old = version(&db, "a", "old", 5);
        let report = db.cull(policy(15, 1), &[old]).unwrap();
        assert_eq!(report.versions_removed, 0);
        assert_eq!(report.versions_in_use, 1);
        assert_eq!(ids(&db), [newest, old]);
        assert_eq!(blob_count(&db), 2);
    }

    #[test]
    fn shared_files_stay_while_any_version_uses_them() {
        let db = ContentDb::open_in_memory().unwrap();
        let (cid, _) = db.store_blob(&blake2b256(b"shared"), b"shared").unwrap();
        let keep = db.create_version("a", "keep").unwrap();
        let drop = db.create_version("a", "drop").unwrap();
        for v in [keep, drop] {
            db.add_manifest_entry(v, "shared.txt", cid).unwrap();
        }
        db.connection()
            .execute(
                "UPDATE ContentVersion SET LastUsed = datetime('2020-01-01') WHERE Id = ?1",
                [drop],
            )
            .unwrap();

        db.cull(policy(15, 1), &[]).unwrap();
        assert_eq!(ids(&db), [keep]);
        assert_eq!(db.read_blob(cid).unwrap(), b"shared");
    }

    #[test]
    fn interrupted_downloads_expire_but_fresh_ones_keep_their_blobs() {
        let db = ContentDb::open_in_memory().unwrap();
        let (fresh_blob, _) = db.store_blob(&blake2b256(b"fresh"), b"fresh").unwrap();
        let (old_blob, _) = db.store_blob(&blake2b256(b"old"), b"old").unwrap();
        let (orphan, _) = db.store_blob(&blake2b256(b"orphan"), b"orphan").unwrap();

        let fresh = db.begin_interrupted_download().unwrap();
        db.link_interrupted_blob(fresh, fresh_blob).unwrap();
        let old = db.begin_interrupted_download().unwrap();
        db.link_interrupted_blob(old, old_blob).unwrap();
        db.connection()
            .execute(
                "UPDATE InterruptedDownload SET Added = datetime('now', '-3 days') WHERE Id = ?1",
                [old],
            )
            .unwrap();

        let report = db.cull(policy(15, 3), &[]).unwrap();
        assert_eq!(report.interrupted_removed, 1);
        assert_eq!(
            report.blobs_removed, 2,
            "old interrupted blob and the unreferenced one"
        );
        assert!(db.read_blob(fresh_blob).is_ok());
        assert!(db.read_blob(old_blob).is_err());
        assert!(db.read_blob(orphan).is_err());
    }

    #[test]
    fn a_download_started_a_moment_ago_survives_clear_everything() {
        let db = ContentDb::open_in_memory().unwrap();
        let (blob, _) = db.store_blob(&blake2b256(b"fresh"), b"fresh").unwrap();
        let fresh = db.begin_interrupted_download().unwrap();
        db.link_interrupted_blob(fresh, blob).unwrap();

        // `cleanup_all` uses zero hours; the row is younger than the grace period.
        let report = db
            .cull(
                CullPolicy {
                    max_versions: 0,
                    max_versions_per_fork: 0,
                    interrupted_keep_hours: 0,
                },
                &[],
            )
            .unwrap();
        assert_eq!(report.interrupted_removed, 0);
        assert!(
            db.read_blob(blob).is_ok(),
            "the other download keeps its blobs"
        );
    }

    #[test]
    fn an_abandoned_download_is_dropped_and_its_blobs_collected() {
        let db = ContentDb::open_in_memory().unwrap();
        let (blob, _) = db.store_blob(&blake2b256(b"stale"), b"stale").unwrap();
        let stale = db.begin_interrupted_download().unwrap();
        db.link_interrupted_blob(stale, blob).unwrap();
        db.connection()
            .execute(
                "UPDATE InterruptedDownload SET Added = datetime('now', '-3 hours') WHERE Id = ?1",
                [stale],
            )
            .unwrap();

        let report = db
            .cull(
                CullPolicy {
                    max_versions: 0,
                    max_versions_per_fork: 0,
                    interrupted_keep_hours: 0,
                },
                &[],
            )
            .unwrap();
        assert_eq!(report.interrupted_removed, 1);
        assert!(db.read_blob(blob).is_err(), "its blobs are collected");
    }

    #[test]
    fn a_heartbeat_keeps_a_slow_download_alive() {
        let db = ContentDb::open_in_memory().unwrap();
        let (blob, _) = db.store_blob(&blake2b256(b"slow"), b"slow").unwrap();
        let id = db.begin_interrupted_download().unwrap();
        db.link_interrupted_blob(id, blob).unwrap();
        db.connection()
            .execute(
                "UPDATE InterruptedDownload SET Added = datetime('now', '-3 hours') WHERE Id = ?1",
                [id],
            )
            .unwrap();

        assert!(db.touch_interrupted_download(id).unwrap());
        let report = db
            .cull(
                CullPolicy {
                    max_versions: 0,
                    max_versions_per_fork: 0,
                    interrupted_keep_hours: 0,
                },
                &[],
            )
            .unwrap();
        assert_eq!(report.interrupted_removed, 0);
        assert!(db.read_blob(blob).is_ok());
        assert!(!db.touch_interrupted_download(12_345).unwrap());
    }

    #[test]
    fn nothing_to_do_removes_nothing() {
        let db = ContentDb::open_in_memory().unwrap();
        version(&db, "a", "1", 1);
        // An unreferenced blob is only collected when something else was removed (like the official code).
        db.store_blob(&blake2b256(b"loose"), b"loose").unwrap();
        let report = db.cull(CullPolicy::default(), &[]).unwrap();
        assert_eq!(report, CullReport::default());
        assert_eq!(blob_count(&db), 2);
    }

    #[test]
    fn running_clients_are_detected_and_dead_rows_removed() {
        let db = ContentDb::open_in_memory().unwrap();
        let v1 = version(&db, "a", "1", 1);
        let v2 = version(&db, "a", "2", 2);
        let c = db.connection();
        c.execute(
            "INSERT INTO RunningClient VALUES (100, 'C:\\loader.exe', ?1)",
            [v1],
        )
        .unwrap();
        c.execute(
            "INSERT INTO RunningClient VALUES (200, 'C:\\loader.exe', ?1)",
            [v2],
        )
        .unwrap();
        c.execute(
            "INSERT INTO RunningClient VALUES (300, 'C:\\other.exe', ?1)",
            [v2],
        )
        .unwrap();

        // Only pid 100 is alive, and pid 300 was reused by another program.
        let used = db
            .running_client_versions(|pid, module| pid == 100 && module == "C:\\loader.exe")
            .unwrap();
        assert_eq!(used, [v1]);
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM RunningClient", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1);

        // A version a live client uses can't be deleted from under it: the foreign key says so too.
        assert!(c
            .execute("DELETE FROM ContentVersion WHERE Id = ?1", [v1])
            .is_err());
    }

    #[test]
    fn only_a_definitely_dead_process_loses_its_row() {
        assert_eq!(
            running_client_action(ProcessState::Unknown),
            RowAction::Keep,
            "a failed probe is not proof that the client is gone"
        );
        assert_eq!(running_client_action(ProcessState::Alive), RowAction::Keep);
        assert_eq!(running_client_action(ProcessState::Gone), RowAction::Drop);
    }

    #[test]
    fn a_client_that_could_not_be_checked_keeps_its_version() {
        let db = ContentDb::open_in_memory().unwrap();
        let v1 = version(&db, "a", "1", 1);
        db.connection()
            .execute(
                "INSERT INTO RunningClient VALUES (100, 'C:\\loader.exe', ?1)",
                [v1],
            )
            .unwrap();

        let used = db
            .running_client_versions_with(|_, _| ProcessState::Unknown)
            .unwrap();
        assert_eq!(used, [v1]);
        let left: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM RunningClient", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "a row for an unchecked process is kept");

        // And a cull that already forgot about it still refuses to delete the version.
        let report = db.cull(policy(0, 0), &[]).unwrap();
        assert_eq!(report.versions_removed, 0);
        assert_eq!(report.versions_in_use, 1);
        assert_eq!(ids(&db), [v1]);
    }

    #[test]
    fn a_client_that_appears_during_the_cull_is_not_a_failure() {
        let db = ContentDb::open_in_memory().unwrap();
        let newest = version(&db, "a", "new", 0);
        let old = version(&db, "a", "old", 5);
        // The Loader inserts this row after `in_use` was computed.
        db.connection()
            .execute(
                "INSERT INTO RunningClient VALUES (100, 'C:\\loader.exe', ?1)",
                [old],
            )
            .unwrap();

        // The delete really is refused by the foreign key...
        let err = db
            .connection()
            .execute("DELETE FROM ContentVersion WHERE Id = ?1", [old])
            .unwrap_err();
        // ...and cull must read that as "in use", not as a failure.
        assert!(is_constraint_violation(&err), "{err}");

        // Reading RunningClient inside the cull transaction is what keeps the version.
        let report = db.cull(policy(15, 1), &[]).unwrap();
        assert_eq!(report.versions_removed, 0);
        assert_eq!(
            report.versions_in_use, 1,
            "the busy version counts as in use"
        );
        assert_eq!(ids(&db), [newest, old]);
        assert_eq!(blob_count(&db), 2, "nothing was collected");
    }
}
