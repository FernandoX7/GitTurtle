//! Captured repository identity changes. Definitions contain public signer
//! references only; publication uses Git's configuration lock and an atomic rename.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileSigning {
    pub key: Option<String>,
    pub format: Option<String>,
    /// Profiles may require signing, but never disable an existing requirement.
    pub commits: bool,
    pub tags: bool,
    pub annotated_tags: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileIdentity {
    pub name: String,
    pub email: String,
    /// None leaves all inherited signing configuration unchanged.
    pub signing: Option<ProfileSigning>,
}
impl ProfileIdentity {
    pub fn validate(&self) -> Result<()> {
        validate_identity(&self.name, &self.email)?;
        if let Some(signing) = &self.signing {
            if let Some(key) = &signing.key {
                ensure!(
                    !key.trim().is_empty()
                        && key.len() <= 4096
                        && !key.chars().any(char::is_control)
                        && !key.contains("PRIVATE KEY"),
                    "Use an existing signing key ID, public key, or key path; never paste a private key"
                );
            }
            if let Some(format) = &signing.format {
                ensure!(
                    matches!(format.as_str(), "openpgp" | "ssh" | "x509"),
                    "Git signing format must be openpgp, ssh, or x509"
                );
            }
        }
        Ok(())
    }
    pub fn matches(&self, effective: &GitProfile) -> bool {
        self.name == effective.name
            && self.email == effective.email
            && self.signing.as_ref().is_none_or(|signing| {
                signing
                    .key
                    .as_ref()
                    .is_none_or(|key| Some(key) == effective.signing_key.as_ref())
                    && signing.format.as_ref().is_none_or(|format| {
                        format == effective.signing_format.as_deref().unwrap_or("openpgp")
                    })
                    && (!signing.commits || effective.signing)
                    && (!signing.tags || effective.tag_signing)
                    && (!signing.annotated_tags
                        || effective.tag_signing
                        || effective.annotated_tag_signing)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePlan {
    pub identity: ProfileIdentity,
    pub worktree: PathBuf,
    pub config_path: PathBuf,
    pub private_worktree: bool,
    expected: GitProfile,
    original: Option<Vec<u8>>,
}

impl GitRepository {
    /// Read all effective profile keys in one bounded Git invocation. Includes
    /// and worktree overrides retain Git's normal last-value precedence.
    pub(super) fn profile_config(&self) -> Result<GitProfile> {
        let mut command = normal_command(self.target());
        command.env("GIT_OPTIONAL_LOCKS", "0").args(["config", "--null", "--get-regexp", "^(user\\.(name|email|signingkey)|commit\\.gpgsign|tag\\.(gpgsign|forcesignannotated)|gpg\\.format|extensions\\.worktreeconfig)$"]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        if !(output.status.success() || output.status.code() == Some(1)) {
            return Err(git_failure(
                &output.stderr,
                format!(
                    "Unable to read Git identity configuration: {}",
                    reported(&output.stderr).trim()
                ),
            ));
        }
        let mut profile = GitProfile::default();
        for item in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
        {
            let (key, value) = item
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or((item, &b""[..]), |at| (&item[..at], &item[at + 1..]));
            let value = text(value);
            match text(key).to_ascii_lowercase().as_str() {
                "user.name" => profile.name = value,
                "user.email" => profile.email = value,
                "user.signingkey" => profile.signing_key = Some(value),
                "gpg.format" => profile.signing_format = Some(value),
                "commit.gpgsign" => profile.signing = config_bool(Some(&value)),
                "tag.gpgsign" => profile.tag_signing = config_bool(Some(&value)),
                "tag.forcesignannotated" => {
                    profile.annotated_tag_signing = config_bool(Some(&value))
                }
                "extensions.worktreeconfig" => profile.private_worktree = config_bool(Some(&value)),
                _ => {}
            }
        }
        Ok(profile)
    }

    pub fn profile_plan(&self, identity: ProfileIdentity) -> Result<ProfilePlan> {
        identity.validate()?;
        ensure!(!self.bare, "Open a working copy to apply a profile");
        ensure!(
            self.operation_state()?.is_none(),
            "Finish or abort the current Git operation before changing its identity"
        );
        let expected = self.profile()?;
        let (private, common) = self.git_directories()?;
        let config_path = if expected.private_worktree {
            private.join("config.worktree")
        } else {
            common.join("config")
        };
        let original = read_config(&config_path)?;
        Ok(ProfilePlan {
            identity,
            worktree: self.path.clone(),
            config_path,
            private_worktree: expected.private_worktree,
            expected,
            original,
        })
    }

    pub(super) fn execute_profile(&self, plan: &ProfilePlan) -> Result<WriteOutcome> {
        plan.identity.validate()?;
        ensure!(
            self.path == plan.worktree,
            "The target worktree changed. Review the profile again."
        );
        let fresh = self.profile_plan(plan.identity.clone())?;
        ensure!(
            fresh.config_path == plan.config_path
                && fresh.expected == plan.expected
                && fresh.original == plan.original,
            "Git identity or configuration changed after review. Refresh and review the profile again."
        );
        let mut lock_name = plan.config_path.as_os_str().to_owned();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&lock_path)
            .context("Git configuration is locked or unavailable; no profile was applied")?;
        let lock = ConfigLock(lock_path);
        ensure!(
            read_config(&plan.config_path)? == plan.original && self.profile()? == plan.expected,
            "Git configuration changed while acquiring its lock. Review the profile again."
        );
        file.write_all(plan.original.as_deref().unwrap_or_default())?;
        file.sync_all()?;
        drop(file);
        let mut values = vec![
            ("user.name", plan.identity.name.clone()),
            ("user.email", plan.identity.email.clone()),
        ];
        if let Some(signing) = &plan.identity.signing {
            if let Some(key) = &signing.key {
                values.push(("user.signingkey", key.clone()));
            }
            if let Some(format) = &signing.format {
                values.push(("gpg.format", format.clone()));
            }
            if signing.commits {
                values.push(("commit.gpgsign", "true".into()));
            }
            if signing.tags {
                values.push(("tag.gpgsign", "true".into()));
            }
            if signing.annotated_tags {
                values.push(("tag.forceSignAnnotated", "true".into()));
            }
        }
        // Remove direct values first, prune now-empty sections for the keys we
        // touched (and stale GitTurtle comment headers), then append grouped
        // explicit overrides after any include directives so they always win.
        let mut touched_sections = std::collections::BTreeSet::new();
        let mut grouped: Vec<(&str, Vec<(&str, String)>)> = Vec::new();
        for (key, value) in values {
            let mut command = normal_command(self.target());
            command
                .args(["config", "--file"])
                .arg(&lock.0)
                .args(["--unset-all", key]);
            let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
            if !(output.status.success() || output.status.code() == Some(5)) {
                return Err(git_failure(
                    &output.stderr,
                    format!(
                        "Could not prepare profile configuration: {}",
                        reported(&output.stderr)
                    ),
                ));
            }
            let (section, name) = key
                .split_once('.')
                .context("Invalid profile configuration key")?;
            touched_sections.insert(section.to_ascii_lowercase());
            let escaped = value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\t', "\\t");
            if let Some((_, entries)) = grouped.iter_mut().find(|(s, _)| *s == section) {
                entries.push((name, escaped));
            } else {
                grouped.push((section, vec![(name, escaped)]));
            }
        }
        prune_empty_profile_sections(&lock.0, &touched_sections)?;
        let mut overrides = String::from("\n# Identity explicitly applied by GitTurtle\n");
        for (section, entries) in grouped {
            overrides.push_str(&format!("[{section}]\n"));
            for (name, value) in entries {
                overrides.push_str(&format!("\t{name} = \"{value}\"\n"));
            }
        }
        OpenOptions::new()
            .append(true)
            .open(&lock.0)?
            .write_all(overrides.as_bytes())?;
        ensure!(
            read_config(&plan.config_path)? == plan.original && self.profile()? == plan.expected,
            "Git configuration changed externally. The profile was not applied."
        );
        if let Ok(metadata) = fs::metadata(&plan.config_path) {
            fs::set_permissions(&lock.0, metadata.permissions())?;
        }
        OpenOptions::new().read(true).open(&lock.0)?.sync_all()?;
        fs::rename(&lock.0, &plan.config_path).context("Publish reviewed Git profile")?;
        if let Some(parent) = plan.config_path.parent()
            && let Ok(directory) = fs::File::open(parent)
        {
            let _ = directory.sync_all();
        }
        let effective = self.profile().context("Profile was saved, but effective Git identity could not be verified; refresh before another action")?;
        let suffix = if plan.identity.matches(&effective) {
            "Subsequent commits and annotated tags use this Git configuration."
        } else {
            "Effective Git identity differs: an environment or inherited override remains. Review the identity before committing."
        };
        Ok(WriteOutcome {
            message: format!("Profile applied. {suffix}"),
            commit_oid: None,
            up_to_date: false,
        })
    }
}

struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn has_multiline_git_config_syntax(raw: &str) -> bool {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let mut in_quote = false;
        let mut escaped = false;
        for ch in line.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_quote = !in_quote,
                '#' | ';' if !in_quote => break,
                _ => {}
            }
        }
        if escaped || in_quote {
            return true;
        }
    }
    false
}

fn prune_empty_profile_sections(
    path: &Path,
    touched_sections: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let Ok(raw) = String::from_utf8(bytes) else {
        return Ok(());
    };
    if raw.is_empty() || has_multiline_git_config_syntax(&raw) {
        return Ok(());
    }
    const MARKER: &str = "# Identity explicitly applied by GitTurtle";
    // Split into preamble + section blocks (`[section] ...`).
    let mut preamble: Vec<&str> = Vec::new();
    let mut sections: Vec<(String, bool, Vec<&str>)> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[')
            && let Some(close) = trimmed.find(']')
        {
            let header = trimmed[1..close].trim();
            // Only plain sections without subsections (`[user]`, `[gpg]`, etc.)
            let section_name = if !header.contains('"') && !header.chars().any(char::is_whitespace)
            {
                header.to_ascii_lowercase()
            } else {
                String::new()
            };
            let trailing = trimmed[close + 1..].trim();
            let inline_entry =
                !trailing.is_empty() && !trailing.starts_with('#') && !trailing.starts_with(';');
            sections.push((section_name, inline_entry, vec![line]));
        } else if let Some((_, _, block)) = sections.last_mut() {
            block.push(line);
        } else {
            preamble.push(line);
        }
    }
    let is_comment_or_blank = |line: &str| {
        let t = line.trim();
        t.is_empty() || t.starts_with('#') || t.starts_with(';')
    };
    let strip_trailing_marker = |lines: &mut Vec<&str>| -> bool {
        let before = lines.len();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines.last().is_some_and(|line| line.trim() == MARKER) {
            lines.pop();
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            return true;
        }
        // No marker was removed; restore nothing because caller handles layout cleanly.
        lines.len() != before
    };
    let mut pruned = false;
    let mut out: Vec<&str> = preamble;
    for (section_name, inline_entry, block) in sections {
        let has_entry = inline_entry || block.iter().skip(1).any(|line| !is_comment_or_blank(line));
        if !has_entry && touched_sections.contains(&section_name) {
            // Drop this now-empty touched section and any preceding GitTurtle header marker.
            strip_trailing_marker(&mut out);
            pruned = true;
            continue;
        }
        out.extend(block);
    }
    if strip_trailing_marker(&mut out) {
        pruned = true;
    }
    if !pruned {
        return Ok(());
    }
    let mut cleaned = out.join("\n");
    if !cleaned.is_empty() {
        cleaned.push('\n');
    }
    fs::write(path, cleaned.as_bytes())?;
    Ok(())
}

fn read_config(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Profile editing requires a regular Git configuration file; linked configuration files must be edited with Git"
    );
    ensure!(
        metadata.len() <= MAX_CONFIG_BYTES,
        "Git configuration exceeds the 2 MiB profile editing limit"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "Git configuration exceeds the profile editing limit"
    );
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn save_profile_does_not_leave_duplicate_empty_user_sections() {
        let dir = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        let repo = GitRepository::open(dir.path()).unwrap();
        let identity = |name: &str, email: &str| ProfileIdentity {
            name: name.into(),
            email: email.into(),
            signing: None,
        };
        for (name, email) in [
            ("First Author", "first@example.com"),
            ("Second Author", "second@example.com"),
            ("Third Author", "third@example.com"),
        ] {
            let plan = repo.profile_plan(identity(name, email)).unwrap();
            repo.execute_profile(&plan).unwrap();
        }
        let config = fs::read_to_string(dir.path().join(".git/config")).unwrap();
        assert_eq!(
            config.matches("[user]").count(),
            1,
            "expected a single [user] section, got:\n{config}"
        );
        assert_eq!(
            config
                .matches("# Identity explicitly applied by GitTurtle")
                .count(),
            1,
            "expected a single GitTurtle comment, got:\n{config}"
        );
        let profile = repo.profile().unwrap();
        assert_eq!(profile.name, "Third Author");
        assert_eq!(profile.email, "third@example.com");
    }

    #[test]
    fn save_profile_preserves_inline_user_section_entries_and_multiline_configs() {
        let dir = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        let config_path = dir.path().join(".git/config");
        let existing = fs::read_to_string(&config_path).unwrap();
        fs::write(
            &config_path,
            format!("{existing}\n[user] signingkey = KEYID123\n\tname = Old\n\temail = old@example.com\n"),
        )
        .unwrap();

        let repo = GitRepository::open(dir.path()).unwrap();
        let plan = repo
            .profile_plan(ProfileIdentity {
                name: "New Author".into(),
                email: "new@example.com".into(),
                signing: None,
            })
            .unwrap();
        repo.execute_profile(&plan).unwrap();

        let after = fs::read_to_string(&config_path).unwrap();
        assert!(
            after.contains("signingkey = KEYID123"),
            "inline entry on [user] header must be preserved, got:\n{after}"
        );
        let profile = repo.profile().unwrap();
        assert_eq!(profile.name, "New Author");
        assert_eq!(profile.email, "new@example.com");
    }
}
