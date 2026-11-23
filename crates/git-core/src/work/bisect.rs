//! Passive Git bisect inspection and explicit guided bisect actions.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BisectState {
    pub original_ref: String,
    pub current_oid: String,
    pub current_short: String,
    pub bad_oid: Option<String>,
    pub good_oids: Vec<String>,
    pub skipped_oids: Vec<String>,
    pub remaining_commits: Option<usize>,
    pub estimated_steps: Option<usize>,
    pub culprit_oid: Option<String>,
    state_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BisectCommand {
    Start { bad: String, good: String },
    Good { expected: BisectState },
    Bad { expected: BisectState },
    Skip { expected: BisectState },
    Reset { expected: BisectState },
}

impl GitRepository {
    /// Inspect worktree-local bisect state passively. Returns `Ok(None)` when no
    /// bisect session is active. Never runs hooks, filters, or network access.
    pub fn bisect_state(&self) -> Result<Option<BisectState>> {
        if self.bare {
            return Ok(None);
        }
        let directory = if matches!(self.verify_at_path()?, FoundAtPath::Same) {
            self.git_dir().to_owned()
        } else {
            self.git_directories()?.0
        };
        let Some(start_bytes) = integration::operation_file(&directory, Path::new("BISECT_START"))?
        else {
            return Ok(None);
        };
        let original_ref = text(trim_line(&start_bytes));
        let log_bytes =
            integration::operation_file(&directory, Path::new("BISECT_LOG"))?.unwrap_or_default();
        let current_oid = text(trim_line(&run_git(
            self.target(),
            &["rev-parse", "--verify", "HEAD"],
        )?));
        let current_short: String = current_oid.chars().take(7).collect();

        let refs_bytes = run_git(
            self.target(),
            &[
                "for-each-ref",
                "--format=%(objectname) %(refname)",
                "refs/bisect/",
            ],
        )?;
        let mut bad_oid = None;
        let mut good_oids = Vec::new();
        let mut skipped_oids = Vec::new();
        for line in text(&refs_bytes).lines() {
            let Some((oid, name)) = line.split_once(' ') else {
                continue;
            };
            if name == "refs/bisect/bad" {
                bad_oid = Some(oid.to_owned());
            } else if name.starts_with("refs/bisect/good-") {
                good_oids.push(oid.to_owned());
            } else if name.starts_with("refs/bisect/skip-") {
                skipped_oids.push(oid.to_owned());
            }
        }
        good_oids.sort();
        skipped_oids.sort();

        // Compute remaining candidate commits passively when both endpoints exist.
        // Cap the number of good refs passed on argv so a pathological repository
        // with thousands of `refs/bisect/good-*` entries cannot hit E2BIG.
        const MAX_BISECT_GOOD_REFS: usize = 256;
        let mut remaining_commits = None;
        let mut estimated_steps = None;
        let mut culprit_oid = None;
        if let Some(bad) = bad_oid.as_deref()
            && !good_oids.is_empty()
            && good_oids.len() <= MAX_BISECT_GOOD_REFS
        {
            let mut args = vec!["rev-list", "--bisect-all", bad, "--not"];
            let good_refs: Vec<&str> = good_oids.iter().map(String::as_str).collect();
            args.extend(good_refs);
            if let Ok(out) = run_git(self.target(), &args) {
                let candidates: Vec<&str> = std::str::from_utf8(&out)
                    .unwrap_or("")
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                let count = candidates.len();
                let remaining = count.saturating_sub(1);
                remaining_commits = Some(remaining);
                estimated_steps = Some(if remaining == 0 {
                    0
                } else {
                    (usize::BITS - remaining.leading_zeros()) as usize
                });
                if count == 1 {
                    culprit_oid = candidates[0].split_whitespace().next().map(str::to_owned);
                }
            }
        }

        let mut digest = Sha256::new();
        for item in [
            original_ref.as_bytes(),
            current_oid.as_bytes(),
            &log_bytes,
            &refs_bytes,
        ] {
            digest.update((item.len() as u64).to_le_bytes());
            digest.update(item);
        }
        Ok(Some(BisectState {
            original_ref,
            current_oid,
            current_short,
            bad_oid,
            good_oids,
            skipped_oids,
            remaining_commits,
            estimated_steps,
            culprit_oid,
            state_token: crate::lower_hex(digest.finalize()),
        }))
    }

    pub(super) fn execute_bisect(&self, action: &BisectCommand) -> Result<WriteOutcome> {
        ensure!(
            !self.bare,
            "Open a working copy to run Git bisect operations"
        );
        let mut command = normal_command(self.target());
        match action {
            BisectCommand::Start { bad, good } => {
                let bad_oid = self.resolve_bisect_commit(bad, "bad")?;
                let good_oid = self.resolve_bisect_commit(good, "good")?;
                ensure!(
                    bad_oid != good_oid,
                    "Choose two different commits for bad and good."
                );
                ensure!(
                    self.bisect_state()?.is_none(),
                    "A bisect session is already in progress. Reset it before starting a new one."
                );
                let status = self.status()?;
                ensure!(
                    status.operation.is_none() && !status.entries.iter().any(|e| e.conflicted),
                    "Finish the current Git operation and resolve conflicts before starting bisect."
                );
                ensure!(
                    !status.entries.iter().any(|e| !e.untracked),
                    "Commit, stash, or discard local changes before starting bisect."
                );
                // `git bisect start <bad> <good> --`: revisions must precede `--`
                // (arguments after `--` are pathspecs). Both OIDs are validated
                // 40/64-char hex commit IDs.
                command.args(["bisect", "start", &bad_oid, &good_oid, "--"]);
            }
            BisectCommand::Good { expected } => {
                self.validate_bisect_state(expected)?;
                command.args(["bisect", "good"]);
            }
            BisectCommand::Bad { expected } => {
                self.validate_bisect_state(expected)?;
                command.args(["bisect", "bad"]);
            }
            BisectCommand::Skip { expected } => {
                self.validate_bisect_state(expected)?;
                command.args(["bisect", "skip"]);
            }
            BisectCommand::Reset { expected } => {
                self.validate_bisect_state(expected)?;
                command.args(["bisect", "reset"]);
            }
        }
        let output = checked_write_output(command, None, WRITE_TIMEOUT)?;
        let stdout_clean = redact_stream(&text(&output.stdout));
        let stderr_clean = redact_stream(&text(&output.stderr));
        let combined = format!("{stdout_clean}{stderr_clean}").trim().to_owned();
        let message = if combined.is_empty() {
            match action {
                BisectCommand::Start { .. } => "Bisect started".into(),
                BisectCommand::Good { .. } => "Marked commit as good".into(),
                BisectCommand::Bad { .. } => "Marked commit as bad".into(),
                BisectCommand::Skip { .. } => "Skipped commit".into(),
                BisectCommand::Reset { .. } => "Stopped bisect and returned to original ref".into(),
            }
        } else {
            combined
        };
        Ok(WriteOutcome {
            message,
            commit_oid: None,
            up_to_date: false,
        })
    }

    fn resolve_bisect_commit(&self, rev: &str, label: &str) -> Result<String> {
        let trimmed = rev.trim();
        ensure!(
            !trimmed.is_empty()
                && trimmed.len() <= 256
                && !trimmed.starts_with('-')
                && !trimmed.chars().any(char::is_control),
            "Enter a valid {label} commit, branch, or tag."
        );
        let peeled = format!("{trimmed}^{{commit}}");
        let out = run_git(
            self.target(),
            &["rev-parse", "--verify", "--end-of-options", &peeled],
        )
        .with_context(|| format!("Unable to resolve {label} commit “{trimmed}”"))?;
        let oid = text(trim_line(&out));
        validate_oid(&oid)?;
        Ok(oid)
    }

    fn validate_bisect_state(&self, expected: &BisectState) -> Result<()> {
        let current = self
            .bisect_state()?
            .context("No bisect session is currently active")?;
        ensure!(
            current.state_token == expected.state_token,
            "Bisect state changed; refresh before continuing."
        );
        Ok(())
    }
}
