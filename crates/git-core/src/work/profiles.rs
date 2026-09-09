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
        let mut command = normal_command(&self.path);
        command.env("GIT_OPTIONAL_LOCKS", "0").args(["config", "--null", "--get-regexp", "^(user\\.(name|email|signingkey)|commit\\.gpgsign|tag\\.(gpgsign|forcesignannotated)|gpg\\.format|extensions\\.worktreeconfig)$"]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to read Git identity configuration: {}",
            text(&output.stderr).trim()
        );
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
        // Remove direct values first, then append explicit overrides after any
        // include directives. Updating a key in place can leave a later include
        // winning over the chosen profile.
        let mut overrides = String::from("\n# Identity explicitly applied by GitTurtle\n");
        for (key, value) in values {
            let mut command = normal_command(&self.path);
            command
                .args(["config", "--file"])
                .arg(&lock.0)
                .args(["--unset-all", key]);
            let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
            ensure!(
                output.status.success() || output.status.code() == Some(5),
                "Could not prepare profile configuration: {}",
                text(&output.stderr)
            );
            let (section, name) = key
                .split_once('.')
                .context("Invalid profile configuration key")?;
            let value = value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\t', "\\t");
            overrides.push_str(&format!("[{section}]\n\t{name} = \"{value}\"\n"));
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
        })
    }
}

struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
