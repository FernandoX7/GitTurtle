//! Bounded, read-only inputs for native working-tree watch coverage. Git owns
//! index/config inspection; matchers only read local ignore files on demand.

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

pub struct LocalWatchPolicy {
    root: PathBuf,
    tracked: HashSet<PathBuf>,
    tracked_directories: HashSet<PathBuf>,
    excludes: Vec<Gitignore>,
    ignores: HashMap<PathBuf, CachedIgnore>,
    ignore_bytes: usize,
    case_insensitive: bool,
    ignore_files: Vec<PathBuf>,
}

struct CachedIgnore {
    matcher: Gitignore,
    source_bytes: usize,
}

impl GitRepository {
    /// Honors tracked paths even beneath ignored directories, local nested
    /// .gitignore files, common info/exclude, and effective global excludes.
    /// No hooks, filters, object reads, index refresh or networking are needed.
    pub fn local_watch_policy(&self) -> Result<LocalWatchPolicy> {
        let mut command = git_command(&self.path);
        command.args(["ls-files", "--cached", "-z"]);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Read tracked paths for local watching: {}",
            text(&output.stderr).trim()
        );
        ensure!(
            output.stdout.len() <= MAX_POLICY_BYTES,
            "Tracked watch paths exceed the 16 MiB coverage budget"
        );
        let mut tracked = HashSet::new();
        let mut tracked_directories = HashSet::new();
        let mut bytes = output.stdout.len();
        for path in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            ensure!(
                tracked.len() < MAX_TRACKED_PATHS,
                "Tracked watch paths exceed the 100,000-path coverage budget"
            );
            let path = path_from_bytes(path);
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
                if tracked_directories.insert(parent.to_owned()) {
                    bytes = bytes.saturating_add(parent.as_os_str().len());
                    ensure!(
                        bytes <= MAX_POLICY_BYTES,
                        "Tracked directory watch paths exceed the 16 MiB coverage budget"
                    );
                }
            }
            tracked.insert(path);
        }
        let config = |key: &str| -> Result<Option<PathBuf>> {
            let mut command = work::normal_command(&self.path);
            command.arg("--no-optional-locks").args([
                "config",
                if key == "core.ignorecase" {
                    "--bool"
                } else {
                    "--path"
                },
                "--null",
                "--get",
                key,
            ]);
            let output = bounded_output(command, GIT_TIMEOUT)?;
            if output.status.code() == Some(1) {
                return Ok(None);
            }
            ensure!(
                output.status.success(),
                "Read local watch configuration: {}",
                text(&output.stderr).trim()
            );
            Ok(Some(path_from_bytes(
                output.stdout.strip_suffix(&[0]).unwrap_or(&output.stdout),
            )))
        };
        let case_insensitive =
            config("core.ignorecase")?.is_some_and(|value| value == Path::new("true"));
        let (_, common) = self.git_directories()?;
        let global = config("core.excludesFile")?.or_else(|| {
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
            case_insensitive,
            ignore_files: Vec::new(),
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
        Ok(policy)
    }
}

impl LocalWatchPolicy {
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
        self.ignore_bytes = 2 * MAX_IGNORE_BYTES;
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
        self.ignore_bytes = self.ignore_bytes.saturating_sub(released);
    }

    /// Ancestor exclusions cannot be undone by a rule inside an excluded
    /// directory. Tracked descendants remain watched independently of that.
    pub fn includes(&mut self, path: &Path, directory: bool) -> Result<bool> {
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
        self.ignore_bytes = self.ignore_bytes.saturating_add(bytes.len());
        ensure!(
            bytes.len() <= MAX_IGNORE_BYTES && self.ignore_bytes <= MAX_POLICY_BYTES,
            "Ignore rules exceed the 16 MiB local watch coverage budget"
        );
        let mut builder = GitignoreBuilder::new(base);
        builder.case_insensitive(self.case_insensitive)?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            // Git ignores invalid individual patterns; do the same while
            // preserving the valid rules in this file.
            let _ = builder.add_line(Some(path.to_owned()), line);
        }
        Ok(builder.build()?)
    }
}
