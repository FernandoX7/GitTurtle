//! Bounded commit-series correspondence and a separate exact-lease push.
//! range-diff's display output is deliberately never parsed as a protocol.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesChange {
    Unchanged,
    Changed,
    Possible,
    Added,
    Dropped,
}
impl SeriesChange {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unchanged => "Same patch and message",
            Self::Changed => "Changed",
            Self::Possible => "Possible match · same subject",
            Self::Added => "Added / unmatched",
            Self::Dropped => "Dropped / unmatched",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesCommit {
    pub commit: RecoveryCommit,
    patch_id: Option<String>,
    patch_digest: String,
    author: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesRow {
    pub original: Option<usize>,
    pub rewritten: Option<usize>,
    pub change: SeriesChange,
    pub reordered: bool,
    pub ambiguous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteReview {
    pub root: PathBuf,
    pub branch: String,
    pub base: String,
    pub original_head: String,
    pub rewritten_head: String,
    pub original: Vec<SeriesCommit>,
    pub rewritten: Vec<SeriesCommit>,
    pub rows: Vec<SeriesRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeasedPublishPlan {
    pub root: PathBuf,
    pub branch: String,
    pub new_oid: String,
    pub remote: String,
    pub remote_ref: String,
    pub expected_remote_oid: String,
    /// Effective single push URL, captured for stale-configuration refusal.
    pub remote_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicationInspection {
    Ready(LeasedPublishPlan),
    /// New locally available remote history requires another user review and
    /// explicit destination check. It is never a replacement push approval.
    RemoteChanged(RewriteReview),
    AlreadyPublished {
        remote: String,
        remote_ref: String,
        oid: String,
    },
}

fn check_cancelled() -> Result<()> {
    ensure!(
        !authentication::current_control().is_some_and(|control| control.is_cancelled()),
        "Series review cancelled"
    );
    Ok(())
}

fn review_read(root: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    check_cancelled()?;
    let mut command = git_command(root);
    command.args(args);
    Ok(checked_write_output(command, input, GIT_TIMEOUT)?.stdout)
}

impl GitRepository {
    /// All identities are immutable objects resolved before the rewrite. Missing
    /// objects fail explicitly; no reflog guess or automatic object fetch occurs.
    pub fn rewrite_review(
        &self,
        base: &str,
        original_head: &str,
        branch: &str,
    ) -> Result<RewriteReview> {
        validate_oid(base)?;
        validate_oid(original_head)?;
        self.validate_branch(branch)?;
        ensure!(
            self.operation_state()?.is_none(),
            "Finish or abort the rebase before reviewing its rewritten series"
        );
        let rewritten_head = text(trim_line(&review_read(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
            None,
        )?));
        validate_oid(&rewritten_head)?;
        let original = self.review_series(base, original_head).context("Original series is unavailable or outside the bounded linear review; original objects may have been removed")?;
        let rewritten = self.review_series(base, &rewritten_head).context(
            "Rewritten series is unavailable or no longer descends linearly from the captured base",
        )?;
        let rows = correspond(&original, &rewritten);
        Ok(RewriteReview {
            root: self.path.clone(),
            branch: branch.into(),
            base: base.into(),
            original_head: original_head.into(),
            rewritten_head,
            original,
            rewritten,
            rows,
        })
    }

    fn review_series(&self, base: &str, head: &str) -> Result<Vec<SeriesCommit>> {
        let started = Instant::now();
        self.recovery_commit(base)?;
        self.recovery_commit(head)?;
        let output = review_read(
            &self.path,
            &[
                "rev-list",
                "--reverse",
                "--topo-order",
                "--max-count=101",
                &format!("{base}..{head}"),
                "--",
            ],
            None,
        )?;
        let ids: Vec<_> = std::str::from_utf8(&output)?.lines().collect();
        ensure!(
            ids.len() <= MAX_INTERACTIVE_REBASE_COMMITS,
            "Review at most 100 commits per series"
        );
        let mut previous = base.to_owned();
        let mut bytes = 0usize;
        let mut series = Vec::new();
        for oid in ids {
            check_cancelled()?;
            ensure!(
                started.elapsed() < GIT_TIMEOUT,
                "Commit-series review exceeded its deadline"
            );
            let commit = self.recovery_commit(oid)?;
            ensure!(
                commit.parents.as_slice() == [previous.as_str()],
                "Only a linear series directly after the captured base can be matched"
            );
            let patch = review_read(
                &self.path,
                &[
                    "diff-tree",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--no-commit-id",
                    "--binary",
                    "-p",
                    &previous,
                    oid,
                    "--",
                ],
                None,
            )?;
            bytes = bytes
                .saturating_add(patch.len())
                .saturating_add(commit.message.len());
            ensure!(
                bytes <= 32 * 1024 * 1024,
                "Series patches exceed the 32 MiB review budget"
            );
            // Strip only the object-name header when comparing exact diff bytes.
            // Hunk coordinates, whitespace, modes, binary bodies and paths remain.
            let mut digest = Sha256::new();
            for line in patch.split_inclusive(|b| *b == b'\n') {
                if !line.starts_with(b"index ") {
                    digest.update(line);
                }
            }
            let patch_digest = format!("{:x}", digest.finalize());
            let patch_id = if patch.is_empty() {
                None
            } else {
                let id = review_read(&self.path, &["patch-id", "--stable"], Some(patch))?;
                let text = std::str::from_utf8(&id)?;
                let mut lines = text.lines();
                let line = lines.next().context("Patch fingerprint unavailable")?;
                ensure!(
                    lines.next().is_none(),
                    "Unexpected multiple patch fingerprints"
                );
                let oid = line
                    .split_whitespace()
                    .next()
                    .context("Patch fingerprint missing")?;
                validate_oid(oid)?;
                Some(oid.to_owned())
            };
            let author = review_read(
                &self.path,
                &["show", "--no-patch", "--format=%an%x00%ae", oid, "--"],
                None,
            )?;
            previous = oid.into();
            series.push(SeriesCommit {
                commit,
                patch_id,
                patch_digest,
                author,
            });
        }
        ensure!(
            previous == head,
            "The captured base is not an ancestor of this series tip"
        );
        Ok(series)
    }

    fn single_push_url(&self, remote: &str) -> Result<String> {
        self.validate_remote(remote)?;
        let mut command = normal_command(&self.path);
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["remote", "get-url", "--push", "--all", "--", remote]);
        let output = checked_write_output(command, None, GIT_TIMEOUT)?.stdout;
        let urls: Vec<_> = std::str::from_utf8(&output)?.lines().collect();
        ensure!(
            urls.len() == 1 && !urls[0].is_empty() && !urls[0].starts_with('-'),
            "Choose a named remote with exactly one push destination"
        );
        Ok(urls[0].into())
    }

    fn check_review_tip(&self, root: &Path, branch: &str, expected: &str) -> Result<()> {
        ensure!(
            self.path == root,
            "This review belongs to a different worktree"
        );
        self.validate_branch(branch)?;
        validate_oid(expected)?;
        ensure!(
            self.operation_state()?.is_none(),
            "Finish the current Git operation before publication"
        );
        let fresh = text(trim_line(&review_read(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
            None,
        )?));
        ensure!(
            fresh == expected,
            "The local branch changed since review. Review the rewritten series again"
        );
        Ok(())
    }

    /// Explicit network inspection only. The UI must show the destination and
    /// obtain a separate confirmation before execute_leased_publish.
    pub fn leased_publish_plan(
        &self,
        review: &RewriteReview,
        remote: &str,
        remote_branch: &str,
    ) -> Result<LeasedPublishPlan> {
        ensure!(
            review.original_head != review.rewritten_head,
            "This branch has no rewritten tip to publish"
        );
        let plan = self.publication_snapshot(review, remote, remote_branch)?;
        ensure!(
            plan.expected_remote_oid == review.original_head,
            "The remote branch no longer points at the captured original tip. Review its new history before publishing; no force push was attempted"
        );
        Ok(plan)
    }

    pub fn inspect_rewrite_publication(
        &self,
        review: &RewriteReview,
        remote: &str,
        remote_branch: &str,
    ) -> Result<PublicationInspection> {
        let plan = self.publication_snapshot(review, remote, remote_branch)?;
        if plan.expected_remote_oid == plan.new_oid {
            return Ok(PublicationInspection::AlreadyPublished {
                remote: plan.remote,
                remote_ref: plan.remote_ref,
                oid: plan.new_oid,
            });
        }
        if plan.expected_remote_oid != review.original_head {
            let changed = self.rewrite_review(&review.base, &plan.expected_remote_oid, &review.branch)
                .context("The remote moved. Its new series cannot be reviewed from available local objects. Explicitly Fetch this named remote, then check the destination again; merge or unrelated history may require Git's tools")?;
            self.check_review_tip(&review.root, &review.branch, &review.rewritten_head)?;
            return Ok(PublicationInspection::RemoteChanged(changed));
        }
        Ok(PublicationInspection::Ready(plan))
    }

    fn publication_snapshot(
        &self,
        review: &RewriteReview,
        remote: &str,
        remote_branch: &str,
    ) -> Result<LeasedPublishPlan> {
        self.check_review_tip(&review.root, &review.branch, &review.rewritten_head)?;
        self.validate_branch(remote_branch)?;
        let remote_url = self.single_push_url(remote)?;
        let remote_ref = format!("refs/heads/{remote_branch}");
        let mut command = normal_command(&self.path);
        configure_network(&mut command, self)?;
        command.args(["ls-remote", "--refs", "--", &remote_url, &remote_ref]);
        let output = checked_write_output(command, None, NETWORK_TIMEOUT)?.stdout;
        let text = std::str::from_utf8(&output)?;
        let lines: Vec<_> = text.lines().collect();
        ensure!(
            lines.len() == 1,
            "The named remote branch is absent or ambiguous; use ordinary Push to publish a new branch"
        );
        let (expected, reference) = lines[0]
            .split_once('\t')
            .context("Remote returned an invalid reference")?;
        validate_oid(expected)?;
        ensure!(
            reference == remote_ref,
            "Remote returned a different branch"
        );
        self.check_review_tip(&review.root, &review.branch, &review.rewritten_head)?;
        ensure!(
            self.single_push_url(remote)? == remote_url,
            "Remote destination changed during inspection; review it again"
        );
        Ok(LeasedPublishPlan {
            root: self.path.clone(),
            branch: review.branch.clone(),
            new_oid: review.rewritten_head.clone(),
            remote: remote.into(),
            remote_ref,
            expected_remote_oid: expected.into(),
            remote_url,
        })
    }

    pub fn execute_leased_publish(&self, plan: &LeasedPublishPlan) -> Result<WriteOutcome> {
        self.check_review_tip(&plan.root, &plan.branch, &plan.new_oid)?;
        validate_oid(&plan.expected_remote_oid)?;
        let branch = plan
            .remote_ref
            .strip_prefix("refs/heads/")
            .context("Publication must target one branch")?;
        self.validate_branch(branch)?;
        ensure!(
            self.single_push_url(&plan.remote)? == plan.remote_url,
            "Remote destination changed after review. Inspect it again before publishing"
        );
        let mut command = normal_command(&self.path);
        configure_network(&mut command, self)?;
        // Pin the reviewed destination after checking the named remote. Using
        // the name here would let a concurrent config edit retarget the write.
        // Git still runs its normal push hooks and authentication for this URL.
        command.args([
            "-c",
            &format!("remote.{}.mirror=false", plan.remote),
            "-c",
            "push.followTags=false",
            "push",
            "--progress",
            "--porcelain",
            "--no-force",
            "--no-follow-tags",
            "--recurse-submodules=no",
            &format!(
                "--force-with-lease={}:{}",
                plan.remote_ref, plan.expected_remote_oid
            ),
            "--",
            &plan.remote_url,
            &format!("{}:{}", plan.new_oid, plan.remote_ref),
        ]);
        let output = checked_write_output(command, None, NETWORK_TIMEOUT).context("Leased publication did not return success. The lease is never broadened or retried; inspect the remote explicitly before another attempt")?;
        Ok(WriteOutcome {
            message: format!(
                "Published {} to {}/{} with the reviewed exact lease.\n{}{}",
                plan.new_oid,
                plan.remote,
                branch,
                text(&output.stdout),
                text(&output.stderr)
            ),
            commit_oid: None,
        })
    }
}

fn correspond(original: &[SeriesCommit], rewritten: &[SeriesCommit]) -> Vec<SeriesRow> {
    let mut matched = HashMap::<usize, (usize, SeriesChange)>::new();
    let mut used = std::collections::HashSet::new();
    for (new, commit) in rewritten.iter().enumerate() {
        if let Some(old) = original
            .iter()
            .position(|old| old.commit.oid == commit.commit.oid)
        {
            matched.insert(new, (old, SeriesChange::Unchanged));
            used.insert(old);
        }
    }
    // Unique stable patch IDs are the strongest available correspondence after
    // an object rewrite. Duplicate IDs and empty patches are never guessed.
    for (new, commit) in rewritten
        .iter()
        .enumerate()
        .filter(|(new, _)| !matched.contains_key(new))
        .collect::<Vec<_>>()
    {
        let Some(id) = &commit.patch_id else {
            continue;
        };
        let old: Vec<_> = original
            .iter()
            .enumerate()
            .filter(|(index, old)| !used.contains(index) && old.patch_id.as_ref() == Some(id))
            .map(|(index, _)| index)
            .collect();
        let count = rewritten
            .iter()
            .enumerate()
            .filter(|(index, other)| {
                !matched.contains_key(index) && other.patch_id.as_ref() == Some(id)
            })
            .count();
        if old.len() == 1 && count == 1 {
            let old = old[0];
            let change = if original[old].commit.message == commit.commit.message
                && original[old].author == commit.author
                && original[old].patch_digest == commit.patch_digest
            {
                SeriesChange::Unchanged
            } else {
                SeriesChange::Changed
            };
            matched.insert(new, (old, change));
            used.insert(old);
        }
    }
    for (new, commit) in rewritten
        .iter()
        .enumerate()
        .filter(|(new, _)| !matched.contains_key(new))
        .collect::<Vec<_>>()
    {
        let old: Vec<_> = original
            .iter()
            .enumerate()
            .filter(|(index, old)| {
                !used.contains(index)
                    && !commit.commit.subject.is_empty()
                    && old.commit.subject == commit.commit.subject
            })
            .map(|(index, _)| index)
            .collect();
        let count = rewritten
            .iter()
            .enumerate()
            .filter(|(index, other)| {
                !matched.contains_key(index) && other.commit.subject == commit.commit.subject
            })
            .count();
        if old.len() == 1 && count == 1 {
            matched.insert(new, (old[0], SeriesChange::Possible));
            used.insert(old[0]);
        }
    }
    let mut rows = Vec::new();
    for (new, commit) in rewritten.iter().enumerate() {
        if let Some(&(old, change)) = matched.get(&new) {
            let reordered = matched.iter().any(|(other_new, (other_old, _))| {
                (other_new < &new && *other_old > old) || (other_new > &new && *other_old < old)
            });
            rows.push(SeriesRow {
                original: Some(old),
                rewritten: Some(new),
                change,
                reordered,
                ambiguous: change == SeriesChange::Possible,
            });
        } else {
            let ambiguous = original.iter().any(|old| {
                (commit.patch_id.is_some() && commit.patch_id == old.patch_id)
                    || commit.commit.subject == old.commit.subject
            });
            rows.push(SeriesRow {
                original: None,
                rewritten: Some(new),
                change: SeriesChange::Added,
                reordered: false,
                ambiguous,
            });
        }
    }
    for (old, commit) in original.iter().enumerate() {
        if !used.contains(&old) {
            rows.push(SeriesRow {
                original: Some(old),
                rewritten: None,
                change: SeriesChange::Dropped,
                reordered: false,
                ambiguous: rewritten.iter().any(|new| {
                    (commit.patch_id.is_some() && commit.patch_id == new.patch_id)
                        || commit.commit.subject == new.commit.subject
                }),
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    fn commit(oid: char, patch: Option<&str>, subject: &str) -> SeriesCommit {
        SeriesCommit {
            commit: RecoveryCommit {
                oid: oid.to_string().repeat(40),
                parents: vec!["f".repeat(40)],
                subject: subject.into(),
                message: format!("{subject}\n"),
            },
            patch_id: patch.map(str::to_owned),
            patch_digest: "digest".into(),
            author: b"same author".to_vec(),
        }
    }
    #[test]
    fn repeated_patches_and_empty_commits_do_not_invent_correspondence() {
        for patch in [Some("same-patch"), None] {
            let old = [
                commit('a', patch, "Repeated subject"),
                commit('b', patch, "Repeated subject"),
            ];
            let new = [
                commit('c', patch, "Repeated subject"),
                commit('d', patch, "Repeated subject"),
            ];
            let rows = correspond(&old, &new);
            assert_eq!(rows.len(), 4);
            assert!(rows.iter().all(|row| row.ambiguous && (row.original.is_none() || row.rewritten.is_none())));
        }
    }
}
