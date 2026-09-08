//! Explicit local branch, tracking, and remote configuration operations.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchReference {
    pub name: String,
    pub oid: String,
}

/// Read this plan on a worker, then retain it with the user's contextual action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPlan {
    pub name: String,
    pub oid: String,
    pub upstream: Option<String>,
    pub checked_out_in: Vec<PathBuf>,
    pub current_branch: Option<String>,
    pub current_head: Option<String>,
    /// Git's safe-deletion comparison: the available upstream, otherwise HEAD.
    pub merge_target: Option<BranchReference>,
    pub unmerged_commits: Option<u64>,
    root: PathBuf,
    config_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackingBranchPlan {
    pub name: String,
    pub remote_ref: String,
    pub remote_oid: String,
    pub checkout: bool,
    pub current_branch: Option<String>,
    pub current_head: Option<String>,
    root: PathBuf,
    config_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamPlan {
    pub branch: BranchPlan,
    /// Full refs/heads/... or refs/remotes/... name; None removes tracking.
    pub upstream_ref: Option<String>,
    pub upstream_oid: Option<String>,
    config_token: String,
}

/// An effective remote configuration, including the local consequences of its
/// removal. All URLs remain local configuration data; reads never contact them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteConfig {
    pub name: String,
    pub urls: Vec<String>,
    /// Empty means pushes use the fetch URL(s), as normal Git does.
    pub push_urls: Vec<String>,
    pub fetch_refspecs: Vec<String>,
    pub upstream_branches: Vec<String>,
    pub tracking_refs: Vec<BranchReference>,
    snapshot: Option<RemoteSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RemoteSnapshot {
    root: PathBuf,
    config_token: String,
    branch_token: String,
}

impl RemoteConfig {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            fetch_refspecs: vec![format!("+refs/heads/*:refs/remotes/{name}/*")],
            name,
            urls: vec![url.into()],
            push_urls: Vec::new(),
            upstream_branches: Vec::new(),
            tracking_refs: Vec::new(),
            snapshot: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchCommand {
    CreateTracking {
        plan: TrackingBranchPlan,
    },
    Rename {
        plan: BranchPlan,
        new_name: String,
    },
    Delete {
        plan: BranchPlan,
    },
    SetUpstream {
        plan: UpstreamPlan,
    },
    AddRemote {
        remote: RemoteConfig,
    },
    /// Retain the name; URL/refspec lists are replaced as one config transaction.
    /// All other options (proxy, mirror, pruning, transport, etc.) are retained.
    EditRemote {
        expected: RemoteConfig,
        replacement: RemoteConfig,
    },
    RemoveRemote {
        expected: RemoteConfig,
    },
}

impl GitRepository {
    pub fn branch_plan(&self, name: &str) -> Result<BranchPlan> {
        self.validate_branch(name)?;
        let status = self.branch_operation_status()?;
        let reference = format!("refs/heads/{name}");
        let record = self
            .management_ref(&reference)?
            .context("This local branch no longer exists; refresh the branch list.")?;
        let upstream = record.upstream;
        let upstream_target = upstream
            .as_deref()
            .map(|name| self.management_ref(name))
            .transpose()?
            .flatten()
            .map(|record| BranchReference {
                name: record.name,
                oid: record.oid,
            });
        let merge_target = upstream_target.or_else(|| {
            status.head.as_ref().map(|oid| BranchReference {
                name: status
                    .branch
                    .as_ref()
                    .map(|name| format!("refs/heads/{name}"))
                    .unwrap_or_else(|| "HEAD".into()),
                oid: oid.clone(),
            })
        });
        let unmerged_commits = merge_target
            .as_ref()
            .map(|target| {
                let result = run_git(
                    &self.path,
                    &[
                        "rev-list",
                        "--count",
                        &format!("{}..{}", target.oid, record.oid),
                        "--",
                    ],
                )?;
                text(trim_line(&result))
                    .parse::<u64>()
                    .context("Unable to count unmerged branch commits")
            })
            .transpose()?;
        let mut checked_out_in: Vec<_> = self
            .worktrees()?
            .into_iter()
            .filter(|tree| tree.branch.as_deref() == Some(name))
            .map(|tree| tree.path)
            .collect();
        checked_out_in.sort();
        Ok(BranchPlan {
            name: name.into(),
            oid: record.oid,
            upstream,
            checked_out_in,
            current_branch: status.branch,
            current_head: status.head,
            merge_target,
            unmerged_commits,
            root: self.path.clone(),
            config_token: self
                .management_config_token(&format!("^branch\\.{}\\.", regex_literal(name)))?,
        })
    }

    pub fn tracking_branch_plan(
        &self,
        name: &str,
        remote_ref: &str,
        checkout: bool,
    ) -> Result<TrackingBranchPlan> {
        self.validate_branch(name)?;
        self.validate_management_ref(remote_ref, true)?;
        let status = self.branch_operation_status()?;
        ensure!(
            self.management_ref(&format!("refs/heads/{name}"))?
                .is_none(),
            "A local branch named '{name}' already exists."
        );
        let source = self.management_ref(remote_ref)?.context(
            "This remote branch is no longer available locally; refresh the branch list.",
        )?;
        Ok(TrackingBranchPlan {
            name: name.into(),
            remote_ref: remote_ref.into(),
            remote_oid: source.oid,
            checkout,
            current_branch: status.branch,
            current_head: status.head,
            root: self.path.clone(),
            config_token: self.management_config_token("^remote\\.")?,
        })
    }

    pub fn upstream_plan(&self, branch: &str, upstream_ref: Option<&str>) -> Result<UpstreamPlan> {
        let branch = self.branch_plan(branch)?;
        let upstream_oid = if let Some(reference) = upstream_ref {
            self.validate_management_ref(reference, false)?;
            ensure!(
                reference != format!("refs/heads/{}", branch.name),
                "A branch cannot track itself."
            );
            Some(
                self.management_ref(reference)?
                    .context(
                        "This upstream branch no longer exists; refresh and choose another branch.",
                    )?
                    .oid,
            )
        } else {
            None
        };
        Ok(UpstreamPlan {
            branch,
            upstream_ref: upstream_ref.map(str::to_owned),
            upstream_oid,
            config_token: self.management_config_token("^remote\\.")?,
        })
    }

    pub fn remote_configs(&self) -> Result<Vec<RemoteConfig>> {
        let mut command = normal_command(&self.path);
        command.env("GIT_OPTIONAL_LOCKS", "0").args(["remote"]);
        let names = checked_write_output(command, None, GIT_TIMEOUT)?.stdout;
        let names = std::str::from_utf8(&names).context("A remote name is not valid UTF-8")?;
        let branches = self.management_config_entries("^branch\\.")?;
        let remote_entries = self.management_config_entries("^remote\\.")?;
        let branch_token = config_entries_token(&branches);
        let references = self.management_refs("refs/remotes/")?;
        let mut remotes = Vec::new();
        for name in names.lines() {
            validate_remote_name(name)?;
            let prefix = format!("remote.{name}.");
            let entries: Vec<_> = remote_entries
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
                .cloned()
                .collect();
            let values = |suffix: &str| {
                entries
                    .iter()
                    .filter(|(key, _)| key == &format!("{prefix}{suffix}"))
                    .map(|(_, value)| value.clone())
                    .collect()
            };
            let mut upstream_branches: Vec<_> = branches
                .iter()
                .filter_map(|(key, value)| {
                    if value == name {
                        key.strip_prefix("branch.")
                            .and_then(|name| name.strip_suffix(".remote"))
                            .map(str::to_owned)
                    } else {
                        None
                    }
                })
                .collect();
            upstream_branches.sort();
            upstream_branches.dedup();
            remotes.push(RemoteConfig {
                name: name.into(),
                urls: values("url"),
                push_urls: values("pushurl"),
                fetch_refspecs: values("fetch"),
                upstream_branches,
                tracking_refs: Vec::new(),
                snapshot: Some(RemoteSnapshot {
                    root: self.path.clone(),
                    config_token: config_entries_token(&entries),
                    branch_token: branch_token.clone(),
                }),
            });
        }
        for index in 0..remotes.len() {
            remotes[index].tracking_refs = references
                .iter()
                .filter(|reference| {
                    remotes[index]
                        .fetch_refspecs
                        .iter()
                        .any(|spec| refspec_destination_matches(spec, &reference.name))
                        && !remotes.iter().enumerate().any(|(other, remote)| {
                            other != index
                                && remote
                                    .fetch_refspecs
                                    .iter()
                                    .any(|spec| refspec_destination_matches(spec, &reference.name))
                        })
                })
                .map(|reference| BranchReference {
                    name: reference.name.clone(),
                    oid: reference.oid.clone(),
                })
                .collect();
        }
        Ok(remotes)
    }

    pub(super) fn execute_branch(&self, operation: &BranchCommand) -> Result<WriteOutcome> {
        ensure!(
            !self.bare,
            "Open a working copy to manage branches and remotes."
        );
        let mut command = normal_command(&self.path);
        let message = match operation {
            BranchCommand::CreateTracking { plan } => {
                ensure!(
                    plan.root == self.path
                        && self.tracking_branch_plan(
                            &plan.name,
                            &plan.remote_ref,
                            plan.checkout
                        )? == *plan,
                    "The branch or remote configuration changed; review the tracking branch again."
                );
                if plan.checkout {
                    command.args([
                        "switch",
                        "--no-guess",
                        "--no-recurse-submodules",
                        "--create",
                        &plan.name,
                        "--track=direct",
                        &plan.remote_ref,
                    ]);
                } else {
                    command.args([
                        "branch",
                        "--track=direct",
                        "--",
                        &plan.name,
                        &plan.remote_ref,
                    ]);
                }
                format!(
                    "Created '{}' tracking '{}'{}",
                    plan.name,
                    plan.remote_ref.trim_start_matches("refs/remotes/"),
                    if plan.checkout {
                        " and switched to it"
                    } else {
                        ""
                    }
                )
            }
            BranchCommand::Rename { plan, new_name } => {
                self.check_branch_plan(plan)?;
                self.validate_branch(new_name)?;
                ensure!(
                    !plan.checked_out_in.iter().any(|path| path != &self.path),
                    "This branch is checked out in another worktree; rename it there."
                );
                ensure!(
                    self.management_ref(&format!("refs/heads/{new_name}"))?
                        .is_none(),
                    "A branch named '{new_name}' already exists; choose a different name."
                );
                command.args(["branch", "--move", "--", &plan.name, new_name]);
                format!("Renamed '{}' to '{new_name}'", plan.name)
            }
            BranchCommand::Delete { plan } => {
                self.check_branch_plan(plan)?;
                ensure!(
                    plan.checked_out_in.is_empty(),
                    "This branch is checked out in a worktree; switch that worktree to another branch before deleting it."
                );
                ensure!(
                    plan.unmerged_commits == Some(0),
                    "This branch has commits outside its upstream or HEAD; merge or preserve them before deleting it."
                );
                command.args(["branch", "--delete", "--", &plan.name]);
                format!("Deleted local branch '{}'", plan.name)
            }
            BranchCommand::SetUpstream { plan } => {
                ensure!(
                    plan.branch.root == self.path
                        && self.upstream_plan(&plan.branch.name, plan.upstream_ref.as_deref())?
                            == *plan,
                    "The branch or upstream changed; review its tracking relationship again."
                );
                ensure!(
                    !plan
                        .branch
                        .checked_out_in
                        .iter()
                        .any(|path| path != &self.path),
                    "This branch is checked out in another worktree; change its upstream there."
                );
                if let Some(reference) = &plan.upstream_ref {
                    command.args([
                        "branch",
                        "--set-upstream-to",
                        reference,
                        "--",
                        &plan.branch.name,
                    ]);
                    format!(
                        "'{}' now tracks '{}'",
                        plan.branch.name,
                        reference
                            .trim_start_matches("refs/remotes/")
                            .trim_start_matches("refs/heads/")
                    )
                } else {
                    command.args(["branch", "--unset-upstream", "--", &plan.branch.name]);
                    format!("Removed upstream from '{}'", plan.branch.name)
                }
            }
            BranchCommand::AddRemote { remote } => return self.write_remote_config(None, remote),
            BranchCommand::EditRemote {
                expected,
                replacement,
            } => return self.write_remote_config(Some(expected), replacement),
            BranchCommand::RemoveRemote { expected } => {
                self.check_remote_snapshot(expected)?;
                self.ensure_local_remote(expected)?;
                self.ensure_removable_remote(expected)?;
                command.args(["remote", "remove", "--", &expected.name]);
                format!(
                    "Removed remote '{}' and its unshared remote-tracking references",
                    expected.name
                )
            }
        };
        let output = checked_write_output(command, None, WRITE_TIMEOUT)?;
        let diagnostic = text(&output.stderr);
        Ok(WriteOutcome {
            message: if diagnostic.trim().is_empty() {
                message
            } else {
                format!("{message}\n{}", diagnostic.trim())
            },
            commit_oid: None,
        })
    }

    fn branch_operation_status(&self) -> Result<RepositoryStatus> {
        let status = self.status()?;
        ensure!(
            status.operation.is_none() && !status.entries.iter().any(|entry| entry.conflicted),
            "Finish the current Git operation and resolve conflicts before changing branches."
        );
        Ok(status)
    }

    fn check_branch_plan(&self, expected: &BranchPlan) -> Result<()> {
        ensure!(
            expected.root == self.path && self.branch_plan(&expected.name)? == *expected,
            "The branch, its upstream, or its worktree changed; refresh and review the action again."
        );
        Ok(())
    }

    fn validate_management_ref(&self, reference: &str, remote_only: bool) -> Result<()> {
        ensure!(
            reference.starts_with("refs/remotes/")
                || (!remote_only && reference.starts_with("refs/heads/")),
            "Choose an exact local or remote branch reference."
        );
        ensure!(
            reference.len() <= 4096,
            "This branch reference is too long."
        );
        run_git(&self.path, &["check-ref-format", reference])?;
        Ok(())
    }

    fn management_ref(&self, reference: &str) -> Result<Option<ManagementRef>> {
        let record = self
            .management_refs(reference)?
            .into_iter()
            .find(|record| record.name == reference);
        ensure!(
            record.as_ref().is_none_or(|record| !record.symbolic),
            "Choose a branch rather than a symbolic HEAD alias."
        );
        Ok(record)
    }

    fn management_refs(&self, prefix: &str) -> Result<Vec<ManagementRef>> {
        let mut command = normal_command(&self.path);
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args([
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname)%00%(objectname)%00%(upstream)%00%(symref)",
                prefix,
            ]);
        let bytes = checked_write_output(command, None, GIT_TIMEOUT)?.stdout;
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|record| !record.is_empty())
            .map(|record| {
                let fields: Vec<_> = record.split(|byte| *byte == 0).collect();
                ensure!(fields.len() == 4, "Malformed branch configuration record.");
                let strings: Vec<_> = fields
                    .iter()
                    .map(|field| std::str::from_utf8(field))
                    .collect::<std::result::Result<_, _>>()?;
                validate_oid(strings[1])?;
                Ok(ManagementRef {
                    name: strings[0].into(),
                    oid: strings[1].into(),
                    upstream: (!strings[2].is_empty()).then(|| strings[2].into()),
                    symbolic: !strings[3].is_empty(),
                })
            })
            .collect()
    }

    fn management_config_values(&self, key: &str, local_only: bool) -> Result<Vec<String>> {
        let mut command = normal_command(&self.path);
        command.env("GIT_OPTIONAL_LOCKS", "0").arg("config");
        if local_only {
            command.arg("--local");
        }
        command.args(["--null", "--get-all", key]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to read Git configuration: {}",
            text(&output.stderr).trim()
        );
        if output.stdout.is_empty() {
            return Ok(Vec::new());
        }
        output
            .stdout
            .strip_suffix(&[0])
            .unwrap_or(&output.stdout)
            .split(|byte| *byte == 0)
            .map(|bytes| Ok(std::str::from_utf8(bytes)?.to_owned()))
            .collect()
    }

    fn management_config_entries(&self, pattern: &str) -> Result<Vec<(String, String)>> {
        self.management_config_entries_in_scope(pattern, false)
    }

    fn management_config_entries_in_scope(
        &self,
        pattern: &str,
        local_only: bool,
    ) -> Result<Vec<(String, String)>> {
        let mut command = normal_command(&self.path);
        command.env("GIT_OPTIONAL_LOCKS", "0").arg("config");
        if local_only {
            command.arg("--local");
        }
        command.args(["--null", "--get-regexp", pattern]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to inspect Git configuration: {}",
            text(&output.stderr).trim()
        );
        if output.stdout.is_empty() {
            return Ok(Vec::new());
        }
        output
            .stdout
            .strip_suffix(&[0])
            .unwrap_or(&output.stdout)
            .split(|byte| *byte == 0)
            .map(|entry| {
                let entry = std::str::from_utf8(entry)?;
                let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
                Ok((key.to_owned(), value.to_owned()))
            })
            .collect()
    }

    fn management_config_token(&self, pattern: &str) -> Result<String> {
        Ok(config_entries_token(
            &self.management_config_entries(pattern)?,
        ))
    }

    fn check_remote_snapshot(&self, expected: &RemoteConfig) -> Result<()> {
        ensure!(
            expected
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.root == self.path),
            "This remote selection belongs to another repository; refresh it."
        );
        let current = self
            .remote_configs()?
            .into_iter()
            .find(|remote| remote.name == expected.name)
            .context("This remote no longer exists; refresh the remote list.")?;
        ensure!(
            current == *expected,
            "The remote configuration, tracking branches, or remote-tracking references changed; review the action again."
        );
        Ok(())
    }

    fn ensure_local_remote(&self, remote: &RemoteConfig) -> Result<()> {
        for (suffix, expected) in [
            ("url", &remote.urls),
            ("pushurl", &remote.push_urls),
            ("fetch", &remote.fetch_refspecs),
        ] {
            ensure!(
                self.management_config_values(&format!("remote.{}.{suffix}", remote.name), true)?
                    == *expected,
                "This remote is defined in included, global, or worktree configuration; edit it at that configuration source."
            );
        }
        Ok(())
    }

    fn ensure_removable_remote(&self, remote: &RemoteConfig) -> Result<()> {
        let pattern = format!("^remote\\.{}\\.", regex_literal(&remote.name));
        let local = self.management_config_entries_in_scope(&pattern, true)?;
        ensure!(
            remote
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.config_token == config_entries_token(&local)),
            "Some remote options come from included, global, or worktree configuration; remove them at that configuration source first."
        );
        let keys: std::collections::HashSet<_> = remote
            .upstream_branches
            .iter()
            .flat_map(|branch| {
                [
                    format!("branch.{branch}.remote"),
                    format!("branch.{branch}.merge"),
                ]
            })
            .collect();
        let effective = self.management_config_entries("^branch\\.")?;
        let local = self.management_config_entries_in_scope("^branch\\.", true)?;
        let relevant = |entries: Vec<(String, String)>| {
            entries
                .into_iter()
                .filter(|(key, _)| keys.contains(key))
                .collect::<Vec<_>>()
        };
        ensure!(
            relevant(effective) == relevant(local),
            "A tracking relationship is defined in included, global, or worktree configuration; remove it at that configuration source first."
        );
        Ok(())
    }

    fn write_remote_config(
        &self,
        expected: Option<&RemoteConfig>,
        replacement: &RemoteConfig,
    ) -> Result<WriteOutcome> {
        self.validate_remote_configuration(replacement)?;
        if let Some(expected) = expected {
            ensure!(
                expected.name == replacement.name,
                "Keep the remote name when editing its URLs and refspecs."
            );
        }
        let common = path_from_bytes(trim_line(&run_git(
            &self.path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?));
        let transaction = RemoteConfigTransaction::acquire(common.join("config"))?;
        if let Some(expected) = expected {
            self.check_remote_snapshot(expected)?;
            self.ensure_local_remote(expected)?;
        } else {
            ensure!(
                !self
                    .remote_configs()?
                    .iter()
                    .any(|remote| remote.name == replacement.name),
                "A remote named '{}' already exists.",
                replacement.name
            );
        }
        transaction.prepare()?;
        for (suffix, values) in [
            ("url", &replacement.urls),
            ("pushurl", &replacement.push_urls),
            ("fetch", &replacement.fetch_refspecs),
        ] {
            let key = format!("remote.{}.{suffix}", replacement.name);
            let mut clear = normal_command(&self.path);
            clear
                .arg("config")
                .arg("--file")
                .arg(&transaction.lock)
                .args(["--unset-all", &key]);
            let output = bounded_write_output(clear, None, WRITE_TIMEOUT)?;
            ensure!(
                output.status.success() || output.status.code() == Some(5),
                "Unable to prepare remote settings: {}",
                text(&output.stderr).trim()
            );
            for value in values {
                let mut write = normal_command(&self.path);
                write
                    .arg("config")
                    .arg("--file")
                    .arg(&transaction.lock)
                    .args(["--add", "--", &key, value]);
                checked_write_output(write, None, WRITE_TIMEOUT)?;
            }
        }
        transaction.publish()?;
        Ok(WriteOutcome {
            message: format!(
                "{} remote '{}'",
                if expected.is_some() {
                    "Updated"
                } else {
                    "Added"
                },
                replacement.name
            ),
            commit_oid: None,
        })
    }

    fn validate_remote_configuration(&self, remote: &RemoteConfig) -> Result<()> {
        validate_remote_name(&remote.name)?;
        run_git(
            &self.path,
            &[
                "check-ref-format",
                &format!("refs/remotes/{}/branch", remote.name),
            ],
        )?;
        ensure!(!remote.urls.is_empty(), "Enter at least one remote URL.");
        for values in [&remote.urls, &remote.push_urls, &remote.fetch_refspecs] {
            ensure!(
                values.len() <= 32,
                "Use at most 32 URLs or fetch refspecs for one remote."
            );
            for value in values {
                ensure!(
                    !value.trim().is_empty()
                        && value.len() <= 16 * 1024
                        && !value.contains(['\0', '\n', '\r']),
                    "Remote URLs and refspecs must be nonempty, single-line values up to 16 KiB."
                );
            }
        }
        for spec in &remote.fetch_refspecs {
            ensure!(
                !spec.starts_with("+^"),
                "A negative fetch refspec cannot also force updates."
            );
            let spec = spec.strip_prefix('+').unwrap_or(spec);
            if let Some(negative) = spec.strip_prefix('^') {
                ensure!(
                    !negative.contains(':'),
                    "A negative fetch refspec contains only its source reference."
                );
                self.validate_refspec_pattern(negative)?;
            } else {
                let (source, destination) = spec
                    .split_once(':')
                    .map_or((spec, None), |(source, destination)| {
                        (source, Some(destination))
                    });
                self.validate_refspec_pattern(source)?;
                if let Some(destination) = destination {
                    ensure!(
                        destination.starts_with("refs/"),
                        "A fetch destination must be a full refs/... name."
                    );
                    self.validate_refspec_pattern(destination)?;
                    ensure!(
                        source.contains('*') == destination.contains('*'),
                        "A wildcard fetch refspec needs a wildcard on both sides."
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_refspec_pattern(&self, reference: &str) -> Result<()> {
        ensure!(
            !reference.is_empty() && !reference.starts_with('-'),
            "Enter a valid fetch reference pattern."
        );
        run_git(
            &self.path,
            &[
                "check-ref-format",
                "--allow-onelevel",
                "--refspec-pattern",
                reference,
            ],
        )?;
        Ok(())
    }
}

struct ManagementRef {
    name: String,
    oid: String,
    upstream: Option<String>,
    symbolic: bool,
}

fn config_entries_token(entries: &[(String, String)]) -> String {
    let mut digest = Sha256::new();
    for (key, value) in entries {
        digest.update(key.as_bytes());
        digest.update([0]);
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

fn regex_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if matches!(
                character,
                '.' | '[' | ']' | '(' | ')' | '{' | '}' | '*' | '+' | '?' | '^' | '$' | '\\' | '|'
            ) {
                vec!['\\', character]
            } else {
                vec![character]
            }
        })
        .collect()
}

fn refspec_destination_matches(spec: &str, reference: &str) -> bool {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    let Some((_, destination)) = spec.split_once(':') else {
        return false;
    };
    if let Some((prefix, suffix)) = destination.split_once('*') {
        reference.len() >= prefix.len() + suffix.len()
            && reference.starts_with(prefix)
            && reference.ends_with(suffix)
    } else {
        reference == destination
    }
}

/// The shared config lock excludes normal Git config writers. Git edits a
/// private copy before one atomic publication, preserving unrelated settings.
struct RemoteConfigTransaction {
    config: PathBuf,
    lock: PathBuf,
    nested_lock: PathBuf,
    published: bool,
}

impl RemoteConfigTransaction {
    fn acquire(config: PathBuf) -> Result<Self> {
        let mut lock = config.as_os_str().to_os_string();
        lock.push(".lock");
        let lock = PathBuf::from(lock);
        let mut nested_lock = lock.as_os_str().to_os_string();
        nested_lock.push(".lock");
        let nested_lock = PathBuf::from(nested_lock);
        match std::fs::symlink_metadata(&nested_lock) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "A remote-configuration lock already exists; inspect the prior operation before retrying."
            ),
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .context(
                "Git configuration is locked by another operation; let it finish and refresh.",
            )?;
        Ok(Self {
            config,
            lock,
            nested_lock,
            published: false,
        })
    }
    fn prepare(&self) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.config)?;
        ensure!(
            metadata.file_type().is_file() && metadata.len() <= WRITE_OUTPUT_LIMIT as u64,
            "Remote editing requires a regular repository config within the 4 MiB size limit."
        );
        std::fs::copy(&self.config, &self.lock)?;
        Ok(())
    }
    fn publish(mut self) -> Result<()> {
        std::fs::File::open(&self.lock)?.sync_all()?;
        std::fs::rename(&self.lock, &self.config).context(
            "Unable to publish remote settings; the previous configuration was preserved.",
        )?;
        self.published = true;
        Ok(())
    }
}

impl Drop for RemoteConfigTransaction {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_file(&self.nested_lock);
            let _ = std::fs::remove_file(&self.lock);
        }
    }
}
