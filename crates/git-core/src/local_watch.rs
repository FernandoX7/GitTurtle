//! Bounded, read-only inputs for native working-tree watch coverage. Git owns
//! index/config inspection; matchers only read local ignore files on demand.

use crate::history::{ReadEnd, stream_history};
use crate::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
};

const MAX_TRACKED_PATHS: usize = 100_000;
const MAX_POLICY_BYTES: usize = 16 * 1024 * 1024;
const MAX_IGNORE_BYTES: usize = 1024 * 1024;
const MAX_IGNORE_FILES: usize = 16_384;

/// Why working-tree watching degraded to `.git` metadata only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitMetadataOnlyReason {
    TrackedPathLimit,
    TrackedByteLimit,
}

/// Active local filesystem watch coverage for a working copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalWatchCoverage {
    /// Watching both `.git` metadata and working-tree files/directories.
    Full,
    /// Repository exceeds the bounded working-tree policy; watch `.git`
    /// metadata only and refresh working-tree edits on focus or manual refresh.
    GitMetadataOnly { reason: GitMetadataOnlyReason },
}

pub struct LocalWatchPolicy {
    root: PathBuf,
    tracked: HashSet<PathBuf>,
    tracked_directories: HashSet<PathBuf>,
    excludes: Vec<Gitignore>,
    ignores: HashMap<PathBuf, CachedIgnore>,
    ignore_bytes: usize,
    exclude_bytes: usize,
    case_insensitive: bool,
    ignore_files: Vec<PathBuf>,
    coverage: LocalWatchCoverage,
}

struct CachedIgnore {
    matcher: Gitignore,
    source_bytes: usize,
}

impl GitRepository {
    /// Honors tracked paths even beneath ignored directories, local nested
    /// .gitignore files, common info/exclude, and effective global excludes.
    /// No hooks, filters, object reads, index refresh or networking are needed.
    ///
    /// Repositories exceeding the tracked path or byte budget degrade cleanly
    /// to [`LocalWatchCoverage::GitMetadataOnly`] without buffering the full
    /// index output in memory.
    pub fn local_watch_policy(&self) -> Result<LocalWatchPolicy> {
        self.local_watch_policy_with_limits(MAX_TRACKED_PATHS, MAX_POLICY_BYTES)
    }

    fn local_watch_policy_with_limits(
        &self,
        max_tracked_paths: usize,
        max_policy_bytes: usize,
    ) -> Result<LocalWatchPolicy> {
        let mut command = git_command(self.target());
        command.args(["ls-files", "--cached", "-z"]);
        let cancellation = HistoryCancellation::default();
        let mut tracked = HashSet::new();
        let mut tracked_directories = HashSet::new();
        let mut pending = Vec::new();
        let mut raw_bytes = 0usize;
        let mut dir_bytes = 0usize;
        let mut degraded: Option<GitMetadataOnlyReason> = None;

        let ingest_path = |raw: &[u8],
                           tracked: &mut HashSet<PathBuf>,
                           tracked_directories: &mut HashSet<PathBuf>,
                           raw_bytes: usize,
                           dir_bytes: &mut usize,
                           degraded: &mut Option<GitMetadataOnlyReason>|
         -> Result<bool> {
            if raw.is_empty() {
                return Ok(true);
            }
            if tracked.len() >= max_tracked_paths {
                *degraded = Some(GitMetadataOnlyReason::TrackedPathLimit);
                return Ok(false);
            }
            let path = path_from_bytes(raw);
            ensure!(
                !path.is_absolute()
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "Invalid tracked watch path"
            );
            for parent in path
                .ancestors()
                .skip(1)
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                // `ancestors()` walks bottom-up: once a parent is already
                // recorded, all of its ancestors are recorded too.
                if tracked_directories.contains(parent) {
                    break;
                }
                if tracked_directories.insert(parent.to_owned()) {
                    *dir_bytes = dir_bytes.saturating_add(parent.as_os_str().len());
                    if raw_bytes.saturating_add(*dir_bytes) > max_policy_bytes {
                        *degraded = Some(GitMetadataOnlyReason::TrackedByteLimit);
                        return Ok(false);
                    }
                }
            }
            tracked.insert(path);
            Ok(true)
        };

        let end = stream_history(command, None, &cancellation, GIT_TIMEOUT, |chunk| {
            raw_bytes = raw_bytes.saturating_add(chunk.len());
            if raw_bytes.saturating_add(dir_bytes) > max_policy_bytes {
                degraded = Some(GitMetadataOnlyReason::TrackedByteLimit);
                return Ok(false);
            }
            pending.extend_from_slice(chunk);
            let mut consumed = 0usize;
            while let Some(offset) = pending[consumed..].iter().position(|byte| *byte == 0) {
                let record = &pending[consumed..consumed + offset];
                consumed += offset + 1;
                if !ingest_path(
                    record,
                    &mut tracked,
                    &mut tracked_directories,
                    raw_bytes,
                    &mut dir_bytes,
                    &mut degraded,
                )? {
                    return Ok(false);
                }
            }
            if consumed > 0 {
                pending.drain(..consumed);
            }
            Ok(true)
        })
        .context("Read tracked paths for local watching")?;

        if degraded.is_none() && end == ReadEnd::Complete && !pending.is_empty() {
            let _ = ingest_path(
                &pending,
                &mut tracked,
                &mut tracked_directories,
                raw_bytes,
                &mut dir_bytes,
                &mut degraded,
            )?;
        }

        if let Some(reason) = degraded {
            return Ok(LocalWatchPolicy {
                root: self.path.clone(),
                tracked: HashSet::new(),
                tracked_directories: HashSet::new(),
                excludes: Vec::new(),
                ignores: HashMap::new(),
                ignore_bytes: 0,
                exclude_bytes: 0,
                case_insensitive: false,
                ignore_files: Vec::new(),
                coverage: LocalWatchCoverage::GitMetadataOnly { reason },
            });
        }
        ensure!(
            end == ReadEnd::Complete,
            "Read tracked paths for local watching exceeded its 15 second time limit"
        );
        let mut config_cmd = work::normal_command(self.target());
        config_cmd.arg("--no-optional-locks").args([
            "config",
            "--null",
            "--get-regexp",
            r"^core\.(ignorecase|excludesfile)$",
        ]);
        let config_out = bounded_output(config_cmd, GIT_TIMEOUT)?;
        let mut raw_ignorecase: Option<Option<Vec<u8>>> = None;
        let mut raw_excludes: Option<Option<Vec<u8>>> = None;
        if config_out.status.success() {
            for entry in config_out.stdout.split(|byte| *byte == 0) {
                if entry.is_empty() {
                    continue;
                }
                let (key, value) = match entry.iter().position(|byte| *byte == b'\n') {
                    Some(end) => (&entry[..end], Some(&entry[end + 1..])),
                    None => (entry, None),
                };
                match key {
                    b"core.ignorecase" => raw_ignorecase = Some(value.map(<[u8]>::to_vec)),
                    b"core.excludesfile" => raw_excludes = Some(value.map(<[u8]>::to_vec)),
                    _ => {}
                }
            }
        } else if !(config_out.status.code() == Some(1) && config_out.stderr.is_empty()) {
            anyhow::bail!(
                "Read local watch configuration: {}",
                reported(&config_out.stderr).trim()
            );
        }
        let single_config = |flag: &str, key: &str| -> Result<Option<PathBuf>> {
            let mut command = work::normal_command(self.target());
            command
                .arg("--no-optional-locks")
                .args(["config", flag, "--null", "--get", key]);
            let output = bounded_output(command, GIT_TIMEOUT)?;
            if output.status.code() == Some(1) {
                return Ok(None);
            }
            ensure!(
                output.status.success(),
                "Read local watch configuration: {}",
                reported(&output.stderr).trim()
            );
            Ok(Some(path_from_bytes(
                output.stdout.strip_suffix(&[0]).unwrap_or(&output.stdout),
            )))
        };
        let case_insensitive = match raw_ignorecase {
            None => false,
            Some(value) => match super::git_boolean(value.as_deref()) {
                Some(on) => on,
                None => single_config("--bool", "core.ignorecase")?
                    .is_some_and(|value| value == Path::new("true")),
            },
        };
        let common = if matches!(self.verify_at_path()?, super::FoundAtPath::Same) {
            self.common_dir().to_path_buf()
        } else {
            self.git_directories()?.1
        };
        let configured_excludes = match raw_excludes {
            None => None,
            Some(None) => single_config("--path", "core.excludesFile")?,
            Some(Some(bytes))
                if bytes.starts_with(b"~")
                    || bytes.starts_with(b"%(prefix)")
                    || bytes.starts_with(b":(optional)") =>
            {
                single_config("--path", "core.excludesFile")?
            }
            Some(Some(bytes)) if bytes.is_empty() => Some(PathBuf::new()),
            Some(Some(bytes)) => Some(path_from_bytes(&bytes)),
        };
        let global = configured_excludes.or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .map(|path| path.join("git/ignore"))
        });
        let mut policy = LocalWatchPolicy {
            root: self.path.clone(),
            tracked,
            tracked_directories,
            excludes: Vec::new(),
            ignores: HashMap::new(),
            ignore_bytes: 0,
            exclude_bytes: 0,
            case_insensitive,
            ignore_files: Vec::new(),
            coverage: LocalWatchCoverage::Full,
        };
        for path in global.into_iter().chain([common.join("info/exclude")]) {
            let path = if path.is_absolute() {
                path
            } else {
                self.path.join(path)
            };
            let matcher = policy.read_ignore(&self.path, &path)?;
            policy.excludes.push(matcher);
            policy.ignore_files.push(path);
        }
        policy.exclude_bytes = policy.ignore_bytes;
        Ok(policy)
    }
}

impl LocalWatchPolicy {
    pub fn coverage(&self) -> LocalWatchCoverage {
        self.coverage
    }

    pub fn is_git_metadata_only(&self) -> bool {
        matches!(self.coverage, LocalWatchCoverage::GitMetadataOnly { .. })
    }

    pub fn ignore_files(&self) -> &[PathBuf] {
        &self.ignore_files
    }
    pub fn tracked_directories(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.tracked_directories
            .iter()
            .map(|path| self.root.join(path))
    }

    pub fn invalidate_ignores(&mut self) {
        // Discard cached matchers, then read rules lazily as affected paths are
        // reconsidered. This does not traverse the worktree.
        self.ignores.clear();
        self.ignore_bytes = self.exclude_bytes;
    }

    /// Directory teardown also releases cached empty matchers and source-byte
    /// accounting. Temporary directory churn must not exhaust lifetime limits.
    pub fn forget_directory(&mut self, directory: &Path) {
        let mut released = 0;
        self.ignores.retain(|path, cached| {
            if path.starts_with(directory) {
                released += cached.source_bytes;
                false
            } else {
                true
            }
        });
        self.ignore_bytes = self
            .ignore_bytes
            .saturating_sub(released)
            .max(self.exclude_bytes);
    }

    /// Ancestor exclusions cannot be undone by a rule inside an excluded
    /// directory. Tracked descendants remain watched independently of that.
    pub fn includes(&mut self, path: &Path, directory: bool) -> Result<bool> {
        if self.is_git_metadata_only() {
            return Ok(false);
        }
        let relative = path
            .strip_prefix(&self.root)
            .context("Watch path is outside the selected worktree")?;
        if relative.as_os_str().is_empty() {
            return Ok(true);
        }
        if (directory && self.tracked_directories.contains(relative))
            || self.tracked.contains(relative)
        {
            return Ok(true);
        }
        let mut ancestors: Vec<_> = path
            .ancestors()
            .take_while(|path| *path != self.root)
            .collect();
        ancestors.reverse();
        let mut parents = vec![self.root.clone()];
        for candidate in ancestors {
            let candidate_is_dir = candidate != path || directory;
            let mut ignored = false;
            for matcher in &self.excludes {
                let matched = matcher.matched(candidate, candidate_is_dir);
                if !matched.is_none() {
                    ignored = matched.is_ignore();
                }
            }
            for parent in &parents {
                if !self.ignores.contains_key(parent) {
                    ensure!(
                        self.ignores.len() < MAX_IGNORE_FILES,
                        "Ignore rules exceed the local watch coverage budget"
                    );
                    let previous_bytes = self.ignore_bytes;
                    let matcher = self.read_ignore(parent, &parent.join(".gitignore"))?;
                    self.ignores.insert(
                        parent.clone(),
                        CachedIgnore {
                            matcher,
                            source_bytes: self.ignore_bytes - previous_bytes,
                        },
                    );
                }
                let matched = self.ignores[parent]
                    .matcher
                    .matched(candidate, candidate_is_dir);
                if !matched.is_none() {
                    ignored = matched.is_ignore();
                }
            }
            if ignored {
                return Ok(false);
            }
            if candidate_is_dir {
                parents.push(candidate.to_owned());
            }
        }
        Ok(true)
    }

    fn read_ignore(&mut self, base: &Path, path: &Path) -> Result<Gitignore> {
        let local_ignore = path.file_name().is_some_and(|name| name == ".gitignore");
        let metadata = match if local_ignore {
            fs::symlink_metadata(path)
        } else {
            fs::metadata(path)
        } {
            Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => metadata,
            Ok(_) => return Ok(Gitignore::empty()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(Gitignore::empty());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Read ignore rules at {}", path.display()));
            }
        };
        ensure!(
            metadata.len() <= MAX_IGNORE_BYTES as u64,
            "Ignore file exceeds the 1 MiB local watch budget: {}",
            path.display()
        );
        let mut bytes = Vec::new();
        #[cfg(unix)]
        let file = {
            use rustix::fs::{Mode, OFlags};
            let flags = OFlags::RDONLY
                | OFlags::CLOEXEC
                | if local_ignore {
                    OFlags::NOFOLLOW
                } else {
                    OFlags::empty()
                };
            match rustix::fs::open(path, flags, Mode::empty()) {
                Ok(file) => fs::File::from(file),
                Err(
                    rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP,
                ) => return Ok(Gitignore::empty()),
                Err(error) => return Err(error.into()),
            }
        };
        #[cfg(not(unix))]
        let file = fs::File::open(path)?;
        file.take(MAX_IGNORE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let next_bytes = self.ignore_bytes.saturating_add(bytes.len());
        ensure!(
            bytes.len() <= MAX_IGNORE_BYTES && next_bytes <= MAX_POLICY_BYTES,
            "Ignore rules exceed the 16 MiB local watch coverage budget"
        );
        let mut builder = GitignoreBuilder::new(base);
        builder.case_insensitive(self.case_insensitive)?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            // Git ignores invalid individual patterns; do the same while
            // preserving the valid rules in this file.
            let _ = builder.add_line(Some(path.to_owned()), line);
        }
        let matcher = builder.build()?;
        self.ignore_bytes = next_bytes;
        Ok(matcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Watch Test",
                "-c",
                "user.email=watch@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn degrades_to_git_metadata_only_on_path_and_byte_limits_without_retaining_sets() {
        let temp = TempDir::new().unwrap();
        let repo = GitRepository::init(temp.path().join("repo"), "main").unwrap();
        let root = repo.path().to_path_buf();
        for i in 0..8 {
            let sub = root.join(format!("dir_{i}"));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join(format!("file_{i}.txt")), "x\n").unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "initial"]);

        let full = repo.local_watch_policy_with_limits(8, 1024 * 1024).unwrap();
        assert_eq!(full.coverage(), LocalWatchCoverage::Full);
        assert!(!full.is_git_metadata_only());
        assert_eq!(full.tracked.len(), 8);

        let mut by_paths = repo.local_watch_policy_with_limits(4, 1024 * 1024).unwrap();
        assert_eq!(
            by_paths.coverage(),
            LocalWatchCoverage::GitMetadataOnly {
                reason: GitMetadataOnlyReason::TrackedPathLimit,
            }
        );
        assert!(by_paths.is_git_metadata_only());
        assert!(by_paths.tracked.is_empty());
        assert!(by_paths.tracked_directories.is_empty());
        assert!(
            !by_paths
                .includes(&root.join("dir_0/file_0.txt"), false)
                .unwrap()
        );

        let by_bytes = repo.local_watch_policy_with_limits(100, 32).unwrap();
        assert_eq!(
            by_bytes.coverage(),
            LocalWatchCoverage::GitMetadataOnly {
                reason: GitMetadataOnlyReason::TrackedByteLimit,
            }
        );
        assert!(by_bytes.tracked.is_empty());
    }

    #[test]
    fn invalidate_ignores_resets_to_actual_exclude_bytes_and_errors_do_not_leak_budget() {
        let temp = TempDir::new().unwrap();
        let repo = GitRepository::init(temp.path().join("repo"), "main").unwrap();
        let root = repo.path().to_path_buf();
        fs::write(root.join(".gitignore"), "*.tmp\n").unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "initial"]);

        let mut policy = repo.local_watch_policy().unwrap();
        let base_exclude = policy.exclude_bytes;
        assert_eq!(policy.ignore_bytes, base_exclude);
        assert!(policy.includes(&root.join("a.txt"), false).unwrap());
        assert!(!policy.includes(&root.join("foo.tmp"), false).unwrap());
        assert!(policy.ignore_bytes > base_exclude);

        policy.invalidate_ignores();
        assert_eq!(policy.ignore_bytes, base_exclude);
        assert!(policy.ignores.is_empty());

        // Warm root .gitignore again (with an untracked path) so `before` reflects root rules.
        assert!(!policy.includes(&root.join("foo.tmp"), false).unwrap());
        // An oversized local .gitignore fails cleanly without mutating ignore_bytes.
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".gitignore"), vec![b'a'; MAX_IGNORE_BYTES + 16]).unwrap();
        let before = policy.ignore_bytes;
        assert!(policy.includes(&sub.join("b.txt"), false).is_err());
        assert_eq!(policy.ignore_bytes, before);

        // Exceeding MAX_POLICY_BYTES during read_ignore also must not leak budget.
        let sub2 = root.join("sub2");
        fs::create_dir_all(&sub2).unwrap();
        fs::write(sub2.join(".gitignore"), "*.log\n*.bak\n").unwrap();
        policy.ignore_bytes = MAX_POLICY_BYTES - 2;
        assert!(policy.includes(&sub2.join("c.txt"), false).is_err());
        assert_eq!(policy.ignore_bytes, MAX_POLICY_BYTES - 2);
    }
}
