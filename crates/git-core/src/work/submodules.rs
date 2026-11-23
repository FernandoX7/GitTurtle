//! Passive Git submodule inspection and explicit init/update writes.
use super::*;
use std::collections::HashMap;

const MAX_GITMODULES_BYTES: usize = 512 * 1024;
const MAX_SUBMODULES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmoduleState {
    Clean,
    Uninitialized,
    ModifiedCommit,
    Dirty,
    Conflict,
}

impl SubmoduleState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clean => "Clean",
            Self::Uninitialized => "Not initialized",
            Self::ModifiedCommit => "New commit",
            Self::Dirty => "Modified",
            Self::Conflict => "Conflict",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmoduleSummary {
    pub name: String,
    pub path: PathBuf,
    pub url: Option<String>,
    pub index_oid: String,
    pub head_oid: Option<String>,
    pub state: SubmoduleState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmoduleCommand {
    Init {
        path: Option<PathBuf>,
    },
    Update {
        path: Option<PathBuf>,
        init: bool,
        recursive: bool,
    },
}

impl GitRepository {
    /// Passively inspect repository submodules when `.gitmodules` exists as a
    /// regular non-symlink file in the working tree. Never invokes hooks,
    /// clean/smudge filters, or network fetches.
    pub fn submodules(&self) -> Result<Vec<SubmoduleSummary>> {
        if self.bare {
            return Ok(Vec::new());
        }
        let gitmodules_path = self.path.join(".gitmodules");
        let Ok(meta) = std::fs::symlink_metadata(&gitmodules_path) else {
            return Ok(Vec::new());
        };
        if !meta.file_type().is_file() || meta.len() as usize > MAX_GITMODULES_BYTES {
            return Ok(Vec::new());
        }
        let (mode, gitmodules_bytes) =
            read_worktree_file(&self.path, Path::new(".gitmodules"), "100644")?;
        if mode == "120000" || gitmodules_bytes.len() > MAX_GITMODULES_BYTES {
            return Ok(Vec::new());
        }
        let configured = parse_gitmodules(&gitmodules_bytes);
        if configured.is_empty() {
            return Ok(Vec::new());
        }

        // Read gitlink (mode 160000) entries strictly for the paths declared in
        // `.gitmodules` so large monorepos never dump the entire index.
        let mut declared_paths: Vec<&str> = configured
            .keys()
            .filter(|p| validate_path(Path::new(p)).is_ok())
            .map(String::as_str)
            .collect();
        declared_paths.sort_unstable();
        declared_paths.truncate(MAX_SUBMODULES);
        if declared_paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut ls_args: Vec<&str> = Vec::with_capacity(declared_paths.len() + 4);
        ls_args.extend(["ls-files", "--stage", "-z", "--"]);
        ls_args.extend(declared_paths);
        let index_bytes = run_git(self.target(), &ls_args)?;
        let mut gitlinks: Vec<(PathBuf, String, bool)> = Vec::new();
        let mut conflicted_paths = std::collections::HashSet::new();
        for record in index_bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let Some(tab) = record.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let meta_fields: Vec<&[u8]> = record[..tab].split(|b| *b == b' ').collect();
            if meta_fields.len() != 3 || meta_fields[0] != b"160000" {
                continue;
            }
            let path = path_from_bytes(&record[tab + 1..]);
            let oid = text(meta_fields[1]);
            let stage = meta_fields[2];
            if stage != b"0" {
                conflicted_paths.insert(path.clone());
            }
            if !gitlinks.iter().any(|(existing, _, _)| existing == &path) {
                gitlinks.push((path, oid, stage != b"0"));
            }
            if gitlinks.len() >= MAX_SUBMODULES {
                break;
            }
        }

        const MAX_INSPECTED_WORKTREES: usize = 64;
        let inspect_deadline = Instant::now() + Duration::from_millis(1500);
        let mut inspected = 0usize;
        let mut results = Vec::with_capacity(gitlinks.len());
        for (path, index_oid, _) in gitlinks {
            let key = path.to_string_lossy().replace('\\', "/");
            let meta_entry = configured.get(&key);
            let name = meta_entry
                .map(|m| m.name.clone())
                .unwrap_or_else(|| key.clone());
            let url = meta_entry.and_then(|m| m.url.clone());

            let (head_oid, state) = if conflicted_paths.contains(&path) {
                (None, SubmoduleState::Conflict)
            } else if inspected < MAX_INSPECTED_WORKTREES && Instant::now() < inspect_deadline {
                inspected += 1;
                self.inspect_submodule_worktree(&path, &index_oid)
            } else {
                (None, SubmoduleState::Clean)
            };

            results.push(SubmoduleSummary {
                name,
                path,
                url,
                index_oid,
                head_oid,
                state,
            });
        }
        results.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(results)
    }

    fn inspect_submodule_worktree(
        &self,
        rel_path: &Path,
        index_oid: &str,
    ) -> (Option<String>, SubmoduleState) {
        if validate_path(rel_path).is_err() {
            return (None, SubmoduleState::Uninitialized);
        }
        // Reject any symlink component between repository root and submodule directory.
        let mut cursor = self.path.clone();
        for comp in rel_path.components() {
            let Component::Normal(segment) = comp else {
                return (None, SubmoduleState::Uninitialized);
            };
            cursor.push(segment);
            let Ok(meta) = std::fs::symlink_metadata(&cursor) else {
                return (None, SubmoduleState::Uninitialized);
            };
            if !meta.file_type().is_dir() {
                return (None, SubmoduleState::Uninitialized);
            }
        }
        let dot_git = cursor.join(".git");
        let Ok(dot_meta) = std::fs::symlink_metadata(&dot_git) else {
            return (None, SubmoduleState::Uninitialized);
        };
        // Never follow a symlinked `.git` entry or a crafted gitfile pointing
        // outside this parent repository's `.git/modules/` directory.
        if dot_meta.file_type().is_symlink() {
            return (None, SubmoduleState::Uninitialized);
        }
        if dot_meta.file_type().is_file() {
            if dot_meta.len() > 4096 {
                return (None, SubmoduleState::Uninitialized);
            }
            let Ok(content) = std::fs::read(&dot_git) else {
                return (None, SubmoduleState::Uninitialized);
            };
            let Ok(line) = std::str::from_utf8(&content) else {
                return (None, SubmoduleState::Uninitialized);
            };
            let Some(raw_target) = line.trim().strip_prefix("gitdir:") else {
                return (None, SubmoduleState::Uninitialized);
            };
            let target_path = Path::new(raw_target.trim());
            let resolved = if target_path.is_absolute() {
                target_path.to_path_buf()
            } else {
                cursor.join(target_path)
            };
            let Ok(canonical_target) = std::fs::canonicalize(&resolved) else {
                return (None, SubmoduleState::Uninitialized);
            };
            let allowed_common = std::fs::canonicalize(self.common_dir().join("modules")).ok();
            let allowed_git = std::fs::canonicalize(self.git_dir().join("modules")).ok();
            let inside_parent = allowed_common
                .as_ref()
                .is_some_and(|base| canonical_target.starts_with(base))
                || allowed_git
                    .as_ref()
                    .is_some_and(|base| canonical_target.starts_with(base));
            if !inside_parent {
                return (None, SubmoduleState::Uninitialized);
            }
        } else if !dot_meta.file_type().is_dir() {
            return (None, SubmoduleState::Uninitialized);
        }
        let sub_target = GitTarget::unconfined(&cursor);
        let Ok(head_bytes) = run_git(sub_target, &["rev-parse", "--verify", "HEAD"]) else {
            return (None, SubmoduleState::Uninitialized);
        };
        let head_oid = text(trim_line(&head_bytes));
        if head_oid != index_oid {
            return (Some(head_oid), SubmoduleState::ModifiedCommit);
        }
        // Passive clean/dirty check inside the initialized submodule.
        if let Ok(mut cmd) = passive_status_command(sub_target) {
            cmd.args(["status", "--porcelain=v1", "--untracked-files=no", "-z"]);
            if let Ok(out) = bounded_output(cmd, Duration::from_secs(2))
                && out.status.success()
                && !out.stdout.is_empty()
            {
                return (Some(head_oid), SubmoduleState::Dirty);
            }
        }
        (Some(head_oid), SubmoduleState::Clean)
    }

    pub(super) fn execute_submodule(&self, action: &SubmoduleCommand) -> Result<WriteOutcome> {
        ensure!(!self.bare, "Open a working copy to manage Git submodules");
        let status = self.status()?;
        ensure!(
            status.operation.is_none() && !status.entries.iter().any(|e| e.conflicted),
            "Finish the current Git operation and resolve conflicts before updating submodules."
        );
        let mut command = normal_command(self.target());
        let timeout = match action {
            SubmoduleCommand::Init { path } => {
                command.args(["submodule", "init"]);
                if let Some(rel) = path {
                    validate_path(rel)?;
                    command.arg("--").arg(rel);
                }
                WRITE_TIMEOUT
            }
            SubmoduleCommand::Update {
                path,
                init,
                recursive,
            } => {
                configure_network(&mut command, self)?;
                command.args(["-c", "protocol.file.allow=user", "submodule", "update"]);
                if *init {
                    command.arg("--init");
                }
                if *recursive {
                    command.arg("--recursive");
                }
                if let Some(rel) = path {
                    validate_path(rel)?;
                    command.arg("--").arg(rel);
                }
                NETWORK_TIMEOUT
            }
        };
        let output = checked_write_output(command, None, timeout)?;
        let stdout_clean = redact_stream(&text(&output.stdout));
        let stderr_clean = redact_stream(&text(&output.stderr));
        let combined = format!("{stdout_clean}{stderr_clean}").trim().to_owned();
        Ok(WriteOutcome {
            message: if combined.is_empty() {
                match action {
                    SubmoduleCommand::Init { .. } => "Submodule initialized".into(),
                    SubmoduleCommand::Update { .. } => "Submodule updated".into(),
                }
            } else {
                combined
            },
            commit_oid: None,
            up_to_date: false,
        })
    }
}

struct GitmoduleEntry {
    name: String,
    url: Option<String>,
}

fn parse_gitmodules(bytes: &[u8]) -> HashMap<String, GitmoduleEntry> {
    let text_body = String::from_utf8_lossy(bytes);
    let mut by_path = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_path: Option<String> = None;
    let mut current_url: Option<String> = None;

    let flush = |by_path: &mut HashMap<String, GitmoduleEntry>,
                 name: &mut Option<String>,
                 path: &mut Option<String>,
                 url: &mut Option<String>| {
        if let (Some(n), Some(p)) = (name.take(), path.take()) {
            let normalized = p
                .trim_start_matches("./")
                .trim_end_matches('/')
                .replace('\\', "/");
            by_path.insert(
                normalized,
                GitmoduleEntry {
                    name: n,
                    url: url.take(),
                },
            );
        } else {
            *name = None;
            *path = None;
            *url = None;
        }
    };

    for raw in text_body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            flush(
                &mut by_path,
                &mut current_name,
                &mut current_path,
                &mut current_url,
            );
            let inner = &line[1..line.len() - 1].trim();
            if let Some(rest) = inner.strip_prefix("submodule")
                && let Some(first_quote) = rest.find('"')
                && let Some(last_quote) = rest.rfind('"')
                && last_quote > first_quote
            {
                current_name = Some(rest[first_quote + 1..last_quote].to_owned());
            }
            continue;
        }
        if current_name.is_none() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            if key.eq_ignore_ascii_case("path") {
                current_path = Some(val.to_owned());
            } else if key.eq_ignore_ascii_case("url") {
                current_url = Some(val.to_owned());
            }
        }
    }
    flush(
        &mut by_path,
        &mut current_name,
        &mut current_path,
        &mut current_url,
    );
    by_path
}
