//! PROGRESS UNTIL VERIFIED — Stage 2: attempt progress ledger and
//! repetition detector.
//!
//! Kernel-owned, pure, deterministic; no model involvement, no strategy
//! selection, no enforcement. Stage 2 only OBSERVES and RECORDS:
//!
//! - every pipeline-run task is an attempt identified by its payload
//!   fingerprint (BLAKE3 of the payload text as given by the operator);
//! - attempts with the same payload fingerprint form one group;
//! - within a group:
//!   * the first attempt is the Baseline;
//!   * a Completed attempt is ProgressVerifiedSuccess;
//!   * a Failed attempt whose failure reason was never seen earlier in
//!     the group is ProgressNewFailure (new information);
//!   * a Failed attempt whose failure reason is byte-identical to an
//!     earlier failed attempt is a Repetition: same input, same failure,
//!     no new information and no new strategy — not progress.
//!
//! Known v1 limitations (documented, deliberate):
//! - failure signature comparison is exact string equality (no
//!   normalization of volatile fragments);
//! - the kernel/binary version is not part of the fingerprint: after a
//!   kernel change the same payload may legitimately behave differently.
//!   Stage 2 detects only; it never blocks execution, so a false
//!   repetition flag cannot prevent work — it is an audit observation.
//! - tasks without a readable `pipeline_input.<task>.txt` artifact
//!   (e.g. planner-simulated tasks) are excluded from grouping.

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;

/// BLAKE3 fingerprint of the task payload (the task definition as given).
pub fn payload_fingerprint(payload: &str) -> String {
    blake3::hash(payload.as_bytes()).to_hex().to_string()
}

/// Terminal outcome of one attempt, derived from canonical events only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// At least one STEP_FAILED observed.
    Failed,
    /// STEP_COMPLETED observed and no STEP_FAILED.
    Completed,
    /// Neither observed (incomplete / empty lifecycle).
    Inconclusive,
}

/// One attempt = one task execution with a known payload fingerprint.
#[derive(Debug, Clone)]
pub struct Attempt {
    pub task_id: String,
    pub first_generation: i64,
    pub payload_fingerprint: String,
    pub outcome: AttemptOutcome,
    /// Reason of the FIRST terminal STEP_FAILED, when any.
    pub failure_reason: Option<String>,
}

/// Progress verdict for an attempt within its fingerprint group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// First attempt of the group — the reference point.
    Baseline,
    /// Completed attempt: verified success is progress.
    ProgressVerifiedSuccess,
    /// Failed attempt with a previously unseen failure reason: new
    /// information about the task (progress by distinction).
    ProgressNewFailure,
    /// Failed attempt whose failure reason is byte-identical to an
    /// earlier failed attempt in the group: no new information, no new
    /// strategy — repetition, not progress.
    Repetition { of_task: String },
    /// Lifecycle observed but neither completed nor failed.
    Inconclusive,
}

/// Whole-log progress assessment.
#[derive(Debug, Default)]
pub struct ProgressReport {
    pub attempts: Vec<Attempt>,
    /// (task_id, verdict) in attempt order.
    pub verdicts: Vec<(String, Verdict)>,
}

impl ProgressReport {
    pub fn repetitions(&self) -> Vec<&(String, Verdict)> {
        self.verdicts
            .iter()
            .filter(|(_, v)| matches!(v, Verdict::Repetition { .. }))
            .collect()
    }
}

/// Extract the terminal failure reason of a task from its first
/// STEP_FAILED event payload (`reason` field), if any.
fn first_failure_reason(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload FROM event_log
             WHERE task_id = ?1 AND event_type = 'STEP_FAILED'
             ORDER BY system_generation ASC, sequence_in_unit ASC LIMIT 1",
            [task_id],
            |r| r.get(0),
        )
        .ok();
    match payload {
        None => Ok(None),
        Some(p) => {
            let v: serde_json::Value = serde_json::from_str(&p).map_err(|e| {
                anyhow!("progress: STEP_FAILED payload not JSON for {task_id}: {e}")
            })?;
            Ok(v.get("reason").and_then(|r| r.as_str()).map(str::to_string))
        }
    }
}

fn task_outcome(conn: &Connection, task_id: &str) -> Result<AttemptOutcome> {
    let failed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM event_log WHERE task_id = ?1 AND event_type = 'STEP_FAILED'",
        [task_id],
        |r| r.get(0),
    )?;
    if failed > 0 {
        return Ok(AttemptOutcome::Failed);
    }
    let completed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM event_log WHERE task_id = ?1 AND event_type = 'STEP_COMPLETED'",
        [task_id],
        |r| r.get(0),
    )?;
    if completed > 0 {
        return Ok(AttemptOutcome::Completed);
    }
    Ok(AttemptOutcome::Inconclusive)
}

/// Load all attempts: every task with canonical events whose payload file
/// `pipeline_input.<task>.txt` is readable under `artifacts_dir`. Tasks
/// without that file (e.g. planner-simulated lifecycles) are excluded.
pub fn load_attempts(conn: &Connection, artifacts_dir: &Path) -> Result<Vec<Attempt>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, MIN(system_generation) FROM event_log GROUP BY task_id ORDER BY 2 ASC",
    )?;
    let tasks: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut attempts = Vec::new();
    for (task_id, first_generation) in tasks {
        let payload_path = artifacts_dir.join(format!("pipeline_input.{task_id}.txt"));
        let payload = match std::fs::read_to_string(&payload_path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let outcome = task_outcome(conn, &task_id)?;
        let failure_reason = if outcome == AttemptOutcome::Failed {
            first_failure_reason(conn, &task_id)?
        } else {
            None
        };
        attempts.push(Attempt {
            task_id,
            first_generation,
            payload_fingerprint: payload_fingerprint(&payload),
            outcome,
            failure_reason,
        });
    }
    Ok(attempts)
}

/// Assess progress verdicts for a set of attempts (pure).
pub fn assess(mut attempts: Vec<Attempt>) -> Vec<(String, Verdict)> {
    attempts.sort_by(|a, b| {
        a.first_generation
            .cmp(&b.first_generation)
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    // fingerprint -> (first seen?, failure reasons seen so far with origin)
    let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut seen_groups: BTreeMap<String, bool> = BTreeMap::new();
    let mut verdicts = Vec::new();
    for a in &attempts {
        let is_first = !seen_groups.contains_key(&a.payload_fingerprint);
        seen_groups.insert(a.payload_fingerprint.clone(), true);
        let verdict = match &a.outcome {
            AttemptOutcome::Completed => Verdict::ProgressVerifiedSuccess,
            AttemptOutcome::Inconclusive => Verdict::Inconclusive,
            AttemptOutcome::Failed => {
                if is_first {
                    Verdict::Baseline
                } else {
                    let prior = groups
                        .get(&a.payload_fingerprint)
                        .cloned()
                        .unwrap_or_default();
                    match a
                        .failure_reason
                        .as_ref()
                        .and_then(|reason| prior.iter().find(|(t, r)| r == reason))
                    {
                        Some((origin, _)) => Verdict::Repetition {
                            of_task: origin.clone(),
                        },
                        None => Verdict::ProgressNewFailure,
                    }
                }
            }
        };
        if let (AttemptOutcome::Failed, Some(reason)) = (&a.outcome, &a.failure_reason) {
            groups
                .entry(a.payload_fingerprint.clone())
                .or_default()
                .push((a.task_id.clone(), reason.clone()));
        }
        verdicts.push((a.task_id.clone(), verdict));
    }
    verdicts
}

/// Full assessment over a database + payload artifacts directory.
pub fn assess_db(conn: &Connection, artifacts_dir: &Path) -> Result<ProgressReport> {
    let attempts = load_attempts(conn, artifacts_dir)?;
    let verdicts = assess(attempts.clone());
    Ok(ProgressReport { attempts, verdicts })
}

/// Pre-execution check for `pipeline-run`: does this exact payload already
/// have a failed attempt with a byte-identical failure reason? Returns the
/// earliest matching (task_id, reason). Detect-only: callers must not use
/// this to block execution in Stage 2.
pub fn prior_failure_matches(
    conn: &Connection,
    artifacts_dir: &Path,
    payload: &str,
) -> Result<Option<(String, String)>> {
    let fp = payload_fingerprint(payload);
    let attempts = load_attempts(conn, artifacts_dir)?;
    Ok(attempts
        .iter()
        .filter(|a| a.payload_fingerprint == fp && a.outcome == AttemptOutcome::Failed)
        .filter_map(|a| {
            a.failure_reason
                .as_ref()
                .map(|r| (a.task_id.clone(), r.clone()))
        })
        .next())
}

/// Task-level terminal taxonomy (PROGRESS UNTIL VERIFIED stage 3).
///
/// v1 derivation rules (documented, deliberately conservative):
/// - VerifiedSuccess: the group contains a Completed attempt —
///   kernel-owned verification passed for this task definition.
/// - NoVerifiedPathFound: every attempt in the group failed AND the
///   latest attempt is a Repetition — no new information and no new
///   strategy were brought to bear; the honest state is "no verified
///   path found", NOT "unsolvable" and NOT "LLM said it cannot".
/// - VerifiedFailure: a POSITIVE unsatisfiability proof — not
///   derivable in v1 (needs the stage-5 verifier-gap machinery); the
///   variant exists so the taxonomy is complete.
/// - InProgress: anything else (new failures still carry information).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum TerminalTaxonomy {
    VerifiedSuccess,
    VerifiedFailure,
    NoVerifiedPathFound,
    InProgress,
}

/// Assess the terminal taxonomy for the fingerprint group containing
/// `task_id`. Returns (fingerprint, taxonomy, basis).
pub fn terminal_assessment_for_task(
    conn: &Connection,
    artifacts_dir: &Path,
    task_id: &str,
) -> Result<Option<(String, TerminalTaxonomy, String)>> {
    let report = assess_db(conn, artifacts_dir)?;
    let attempt = match report.attempts.iter().find(|a| a.task_id == task_id) {
        Some(a) => a,
        None => return Ok(None),
    };
    let fp = attempt.payload_fingerprint.clone();
    let group: Vec<&(String, Verdict)> = report
        .verdicts
        .iter()
        .filter(|(t, _)| {
            report
                .attempts
                .iter()
                .find(|a| &a.task_id == t)
                .map(|a| a.payload_fingerprint == fp)
                .unwrap_or(false)
        })
        .collect();
    if group
        .iter()
        .any(|(_, v)| matches!(v, Verdict::ProgressVerifiedSuccess))
    {
        return Ok(Some((
            fp,
            TerminalTaxonomy::VerifiedSuccess,
            format!("attempt {task_id} completed under kernel-owned verification"),
        )));
    }
    let latest = group.last().map(|(t, v)| (t.clone(), v.clone()));
    match latest {
        Some((t, Verdict::Repetition { of_task })) => Ok(Some((
            fp,
            TerminalTaxonomy::NoVerifiedPathFound,
            format!(
                "repetition exhaustion: latest attempt {t} repeats the failure signature of {of_task}; no new information, no new strategy demonstrated"
            ),
        ))),
        Some((_, Verdict::ProgressNewFailure)) => Ok(Some((
            fp,
            TerminalTaxonomy::InProgress,
            "latest attempt produced new failure information; admissible strategies untried"
                .to_string(),
        ))),
        _ => Ok(Some((
            fp,
            TerminalTaxonomy::InProgress,
            "baseline or inconclusive attempt".to_string(),
        ))),
    }
}

/// Human-readable rendering of the report.
pub fn render(report: &ProgressReport) -> String {
    let mut out = String::new();
    out.push_str("PROGRESS LEDGER (PROGRESS UNTIL VERIFIED — stage 2, detect-only)\n");
    out.push_str(&format!(
        "attempts: {}   repetitions: {}\n\n",
        report.attempts.len(),
        report.repetitions().len()
    ));
    for (task_id, verdict) in &report.verdicts {
        let attempt = report.attempts.iter().find(|a| &a.task_id == task_id);
        let (fp, outcome) = match attempt {
            Some(a) => (
                a.payload_fingerprint[..12].to_string(),
                format!("{:?}", a.outcome),
            ),
            None => ("?".to_string(), "?".to_string()),
        };
        let verdict_str = match verdict {
            Verdict::Baseline => "BASELINE".to_string(),
            Verdict::ProgressVerifiedSuccess => "PROGRESS (verified success)".to_string(),
            Verdict::ProgressNewFailure => "PROGRESS (new failure information)".to_string(),
            Verdict::Repetition { of_task } => {
                format!("REPETITION of {of_task} (NO PROGRESS)")
            }
            Verdict::Inconclusive => "INCONCLUSIVE".to_string(),
        };
        out.push_str(&format!(
            "  task {task_id}  payload:{fp}  outcome:{outcome}  -> {verdict_str}\n"
        ));
        if let (Some(a), Verdict::Repetition { .. }) = (attempt, verdict) {
            if let Some(r) = &a.failure_reason {
                out.push_str(&format!("      failure signature: {r}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::storage;
    use serde_json::json;

    struct DbGuard(std::path::PathBuf);
    impl DbGuard {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test failure")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("dak_progress_test_{name}_{nanos}.db"));
            Self(path)
        }
        fn path_str(&self) -> String {
            self.0.to_str().expect("test failure").to_string()
        }
    }
    impl Drop for DbGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    struct ArtifactsGuard(std::path::PathBuf);
    impl ArtifactsGuard {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test failure")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("dak_progress_art_{name}_{nanos}"));
            std::fs::create_dir_all(&dir).expect("test failure");
            Self(dir)
        }
    }
    impl Drop for ArtifactsGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn insert_event(conn: &Connection, gen: i64, task: &str, event_type: &str, payload: &str) {
        conn.execute(
            "INSERT INTO event_log
             (event_id, system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, ?2, 0, ?3, NULL, ?4, ?5, ?2)",
            rusqlite::params![
                format!("evt_{}_{}", task, gen),
                gen,
                task,
                event_type,
                payload
            ],
        )
        .expect("test failure");
    }

    fn failed_task(conn: &Connection, gen: i64, task: &str, reason: &str) {
        insert_event(conn, gen, task, "STEP_STARTED", "{}");
        insert_event(
            conn,
            gen + 1,
            task,
            "STEP_FAILED",
            &json!({"reason": reason, "outcome": "TerminalFailure"}).to_string(),
        );
    }

    fn write_payload(dir: &ArtifactsGuard, task: &str, payload: &str) {
        std::fs::write(dir.0.join(format!("pipeline_input.{task}.txt")), payload)
            .expect("test failure");
    }

    #[test]
    fn fingerprint_is_deterministic_and_payload_scoped() {
        assert_eq!(payload_fingerprint("abc"), payload_fingerprint("abc"));
        assert_ne!(payload_fingerprint("abc"), payload_fingerprint("abd"));
    }

    /// C4 corpus shape: identical payload + identical failure signature
    /// across attempts 2..n must be flagged REPETITION with zero human
    /// annotation.
    #[test]
    fn identical_failure_across_attempts_is_repetition() {
        let guard = DbGuard::new("repetition");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        let reason =
            "fatal: malformed patch: patch_v1 schema violation: invalid escape at line 1 column 290";
        failed_task(&conn, 10, "task_a", reason);
        failed_task(&conn, 20, "task_b", reason);
        failed_task(&conn, 30, "task_c", reason);
        let art = ArtifactsGuard::new("repetition");
        for t in ["task_a", "task_b", "task_c"] {
            write_payload(&art, t, "same payload text");
        }
        let report = assess_db(&conn, &art.0).expect("test failure");
        let verdicts: Vec<_> = report
            .verdicts
            .iter()
            .map(|(t, v)| (t.clone(), v.clone()))
            .collect();
        assert_eq!(verdicts.len(), 3);
        assert_eq!(verdicts[0].1, Verdict::Baseline);
        assert_eq!(
            verdicts[1].1,
            Verdict::Repetition {
                of_task: "task_a".to_string()
            }
        );
        assert_eq!(
            verdicts[2].1,
            Verdict::Repetition {
                of_task: "task_a".to_string()
            }
        );
        assert_eq!(report.repetitions().len(), 2);
    }

    /// A failed attempt with a NEW failure reason carries new information:
    /// progress by distinction, not repetition.
    #[test]
    fn new_failure_reason_is_progress() {
        let guard = DbGuard::new("new_failure");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        failed_task(&conn, 10, "task_a", "fatal: reason one");
        failed_task(&conn, 20, "task_b", "fatal: reason two");
        let art = ArtifactsGuard::new("new_failure");
        for t in ["task_a", "task_b"] {
            write_payload(&art, t, "same payload text");
        }
        let report = assess_db(&conn, &art.0).expect("test failure");
        assert_eq!(report.verdicts[0].1, Verdict::Baseline);
        assert_eq!(report.verdicts[1].1, Verdict::ProgressNewFailure);
        assert_eq!(report.repetitions().len(), 0);
    }

    /// Settlement corpus shape: a completed attempt is verified progress.
    #[test]
    fn completed_attempt_is_verified_success() {
        let guard = DbGuard::new("verified_success");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        insert_event(&conn, 10, "task_a", "STEP_STARTED", "{}");
        insert_event(&conn, 11, "task_a", "STEP_COMPLETED", "{}");
        let art = ArtifactsGuard::new("verified_success");
        write_payload(&art, "task_a", "settlement payload");
        let report = assess_db(&conn, &art.0).expect("test failure");
        assert_eq!(report.verdicts.len(), 1);
        assert_eq!(report.verdicts[0].1, Verdict::ProgressVerifiedSuccess);
    }

    /// Tasks without a payload artifact (e.g. planner-simulated
    /// lifecycles) are excluded from grouping.
    #[test]
    fn tasks_without_payload_file_are_excluded() {
        let guard = DbGuard::new("excluded");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        failed_task(&conn, 10, "task_a", "fatal: something");
        failed_task(&conn, 20, "task_simulated", "fatal: something");
        let art = ArtifactsGuard::new("excluded");
        write_payload(&art, "task_a", "payload text");
        // no payload file for task_simulated
        let report = assess_db(&conn, &art.0).expect("test failure");
        assert_eq!(report.attempts.len(), 1);
        assert_eq!(report.attempts[0].task_id, "task_a");
    }

    /// Distinct payloads never group together even with identical
    /// failures.
    #[test]
    fn distinct_payloads_are_independent_groups() {
        let guard = DbGuard::new("independent");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        failed_task(&conn, 10, "task_a", "fatal: same reason");
        failed_task(&conn, 20, "task_b", "fatal: same reason");
        let art = ArtifactsGuard::new("independent");
        write_payload(&art, "task_a", "payload one");
        write_payload(&art, "task_b", "payload two");
        let report = assess_db(&conn, &art.0).expect("test failure");
        assert_eq!(report.verdicts[0].1, Verdict::Baseline);
        assert_eq!(report.verdicts[1].1, Verdict::Baseline);
    }

    #[test]
    fn prior_failure_matches_finds_earlier_identical_failure() {
        let guard = DbGuard::new("prior_match");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        failed_task(&conn, 10, "task_a", "fatal: the reason");
        let art = ArtifactsGuard::new("prior_match");
        write_payload(&art, "task_a", "the payload");
        let m = prior_failure_matches(&conn, &art.0, "the payload").expect("test failure");
        assert_eq!(
            m,
            Some(("task_a".to_string(), "fatal: the reason".to_string()))
        );
        let none = prior_failure_matches(&conn, &art.0, "another payload").expect("test failure");
        assert_eq!(none, None);
    }

    /// Stage 3 taxonomy: repetition exhaustion (all attempts failed,
    /// latest repeats the signature) is NO VERIFIED PATH FOUND — not
    /// "unsolvable", not "LLM said it cannot".
    #[test]
    fn repetition_exhaustion_is_no_verified_path_found() {
        let guard = DbGuard::new("nvpf");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        let reason = "fatal: same signature";
        failed_task(&conn, 10, "task_a", reason);
        failed_task(&conn, 20, "task_b", reason);
        let art = ArtifactsGuard::new("nvpf");
        for t in ["task_a", "task_b"] {
            write_payload(&art, t, "same payload");
        }
        let (_, taxonomy, basis) = terminal_assessment_for_task(&conn, &art.0, "task_b")
            .expect("test failure")
            .expect("test failure");
        assert_eq!(taxonomy, TerminalTaxonomy::NoVerifiedPathFound);
        assert!(basis.contains("repetition exhaustion"), "got: {basis}");
    }

    /// Stage 3 taxonomy: a completed attempt is VERIFIED SUCCESS.
    #[test]
    fn completed_group_is_verified_success() {
        let guard = DbGuard::new("taxonomy_success");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        insert_event(&conn, 10, "task_a", "STEP_STARTED", "{}");
        insert_event(&conn, 11, "task_a", "STEP_COMPLETED", "{}");
        let art = ArtifactsGuard::new("taxonomy_success");
        write_payload(&art, "task_a", "payload");
        let (_, taxonomy, _) = terminal_assessment_for_task(&conn, &art.0, "task_a")
            .expect("test failure")
            .expect("test failure");
        assert_eq!(taxonomy, TerminalTaxonomy::VerifiedSuccess);
    }

    /// Stage 3 taxonomy: a NEW failure reason keeps the group IN
    /// PROGRESS (new information; strategies untried).
    #[test]
    fn new_failure_keeps_group_in_progress() {
        let guard = DbGuard::new("taxonomy_new_failure");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        failed_task(&conn, 10, "task_a", "fatal: reason one");
        failed_task(&conn, 20, "task_b", "fatal: reason two");
        let art = ArtifactsGuard::new("taxonomy_new_failure");
        for t in ["task_a", "task_b"] {
            write_payload(&art, t, "same payload");
        }
        let (_, taxonomy, basis) = terminal_assessment_for_task(&conn, &art.0, "task_b")
            .expect("test failure")
            .expect("test failure");
        assert_eq!(taxonomy, TerminalTaxonomy::InProgress);
        assert!(basis.contains("new failure information"), "got: {basis}");
    }

    /// Unknown tasks yield no assessment (never a fabricated one).
    #[test]
    fn unknown_task_has_no_assessment() {
        let guard = DbGuard::new("taxonomy_unknown");
        let conn = storage::open_initialized(&guard.path_str()).expect("test failure");
        let art = ArtifactsGuard::new("taxonomy_unknown");
        let r = terminal_assessment_for_task(&conn, &art.0, "ghost").expect("test failure");
        assert!(r.is_none());
    }
}
