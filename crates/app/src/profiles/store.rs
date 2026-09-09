//! Bounded named identities; the existing preferences executor serializes saves.
use anyhow::{Context, Result, ensure};
use gitturtle_core::{GitProfile, ProfileIdentity, ProfileSigning};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 512 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Definition {
    pub id: String,
    pub label: String,
    pub name: String,
    pub email: String,
    pub signing: Option<Signing>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Signing {
    pub key: Option<String>,
    pub format: Option<String>,
    pub commits: bool,
    pub tags: bool,
    #[serde(default)]
    pub annotated_tags: bool,
}
impl Signing {
    pub fn capture(profile: &GitProfile) -> Self {
        Self {
            key: profile.signing_key.clone(),
            format: profile.signing_format.clone(),
            commits: profile.signing,
            tags: profile.tag_signing,
            annotated_tags: profile.annotated_tag_signing,
        }
    }
}
impl Definition {
    pub fn identity(&self) -> ProfileIdentity {
        ProfileIdentity {
            name: self.name.clone(),
            email: self.email.clone(),
            signing: self.signing.as_ref().map(|s| ProfileSigning {
                key: s.key.clone(),
                format: s.format.clone(),
                commits: s.commits,
                tags: s.tags,
                annotated_tags: s.annotated_tags,
            }),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 80
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid profile identifier"
        );
        ensure!(
            !self.label.trim().is_empty()
                && self.label.len() <= 80
                && !self.label.chars().any(char::is_control),
            "Choose a profile name of 1–80 bytes without control characters"
        );
        self.identity().validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Assignment {
    worktree: StoredPath,
    profile: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum StoredPath {
    Text(String),
    #[cfg(unix)]
    Bytes {
        unix_bytes: Vec<u8>,
    },
}
impl StoredPath {
    fn from_path(path: &Path) -> Self {
        if let Some(value) = path.to_str() {
            return Self::Text(value.into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Bytes {
                unix_bytes: path.as_os_str().as_bytes().into(),
            }
        }
        #[cfg(not(unix))]
        {
            Self::Text(path.to_string_lossy().into())
        }
    }
    fn path(&self) -> PathBuf {
        match self {
            Self::Text(text) => text.into(),
            #[cfg(unix)]
            Self::Bytes { unix_bytes } => {
                use std::os::unix::ffi::OsStringExt;
                std::ffi::OsString::from_vec(unix_bytes.clone()).into()
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Store {
    version: u8,
    pub definitions: Vec<Definition>,
    assignments: Vec<Assignment>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            definitions: Vec::new(),
            assignments: Vec::new(),
        }
    }
}
#[derive(Clone)]
pub(super) enum Mutation {
    Save {
        previous: Option<Definition>,
        definition: Definition,
    },
    Delete(Definition),
    Assign {
        worktree: PathBuf,
        profile: Definition,
    },
}
impl Store {
    pub fn path() -> Result<PathBuf> {
        Ok(crate::preferences::settings_path()?.with_file_name("profiles.json"))
    }
    pub fn load() -> Result<Self> {
        Self::load_at(&Self::path()?)
    }
    fn load_at(path: &Path) -> Result<Self> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Saved profiles exceed the 512 KiB limit"
        );
        let store: Self = serde_json::from_slice(&bytes)
            .context("Read saved profiles; the existing file was preserved")?;
        store.validate()?;
        Ok(store)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Saved profiles use an unsupported version; the file was preserved"
        );
        ensure!(
            self.definitions.len() <= 64 && self.assignments.len() <= 512,
            "Profiles support up to 64 definitions and 512 assignments"
        );
        let mut ids = std::collections::HashSet::new();
        let mut labels = std::collections::HashSet::new();
        for profile in &self.definitions {
            profile.validate()?;
            ensure!(
                ids.insert(&profile.id) && labels.insert(profile.label.to_lowercase()),
                "Profile names and identifiers must be unique"
            );
        }
        for assignment in &self.assignments {
            ensure!(
                assignment.worktree.path().is_absolute() && ids.contains(&assignment.profile),
                "Invalid saved profile assignment"
            );
        }
        Ok(())
    }
    pub fn assigned(&self, worktree: &Path) -> Option<&Definition> {
        let assignment = self
            .assignments
            .iter()
            .find(|a| a.worktree.path() == worktree)?;
        self.definitions.iter().find(|p| p.id == assignment.profile)
    }
    pub fn update(mutation: &Mutation) -> Result<Self> {
        Self::update_at(&Self::path()?, mutation)
    }
    fn update_at(path: &Path, mutation: &Mutation) -> Result<Self> {
        let parent = path
            .parent()
            .context("Profiles have no storage directory")?;
        fs::create_dir_all(parent)?;
        // The OS releases this advisory lock even after abrupt termination.
        // Keep its inode stable; unlinking a lock file could split concurrent
        // application instances across different locks.
        let lock_path = path.with_extension("json.lock");
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock_file = options
            .open(&lock_path)
            .context("Open profile storage lock")?;
        lock_file
            .try_lock()
            .context("Profile storage is busy in another application instance")?;
        let mut store = Self::load_at(path)?;
        match mutation {
            Mutation::Save {
                previous,
                definition,
            } => {
                definition.validate()?;
                let current = store.definitions.iter().position(|p| p.id == definition.id);
                ensure!(
                    current.map(|i| &store.definitions[i]) == previous.as_ref(),
                    "This profile changed elsewhere. Reopen its editor before saving."
                );
                if let Some(index) = current {
                    store.definitions[index] = definition.clone();
                } else {
                    store.definitions.push(definition.clone());
                }
            }
            Mutation::Delete(expected) => {
                ensure!(
                    store.definitions.iter().any(|p| p == expected),
                    "This profile changed elsewhere. Review deletion again."
                );
                store.definitions.retain(|p| p.id != expected.id);
                store.assignments.retain(|a| a.profile != expected.id);
            }
            Mutation::Assign { worktree, profile } => {
                ensure!(
                    worktree.is_absolute() && store.definitions.iter().any(|p| p == profile),
                    "Git configuration was applied, but the saved profile changed. Reopen the picker to review the assignment."
                );
                store.assignments.retain(|a| a.worktree.path() != *worktree);
                store.assignments.push(Assignment {
                    worktree: StoredPath::from_path(worktree),
                    profile: profile.id.clone(),
                });
            }
        }
        store.validate()?;
        let bytes = serde_json::to_vec_pretty(&store)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Saved profiles exceed the 512 KiB limit"
        );
        crate::preferences::atomic_write(path, &bytes).context("Save profiles")?;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition(id: &str) -> Definition {
        Definition {
            id: id.into(),
            label: id.into(),
            name: "Profile Author".into(),
            email: format!("{id}@example.invalid"),
            signing: None,
        }
    }
    fn path() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("gitturtle-profiles-{}-{nonce}", std::process::id()))
            .join("profiles.json")
    }
    #[test]
    fn restart_assignments_edit_delete_and_stale_saves_preserve_other_profiles() {
        let path = path();
        let one = definition("Personal");
        let two = definition("Work");
        for profile in [&one, &two] {
            Store::update_at(
                &path,
                &Mutation::Save {
                    previous: None,
                    definition: profile.clone(),
                },
            )
            .unwrap();
        }
        let worktree = path.parent().unwrap().join("worktree");
        Store::update_at(
            &path,
            &Mutation::Assign {
                worktree: worktree.clone(),
                profile: one.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            Store::load_at(&path).unwrap().assigned(&worktree),
            Some(&one)
        );
        let mut edited = one.clone();
        edited.email = "new@example.invalid".into();
        Store::update_at(
            &path,
            &Mutation::Save {
                previous: Some(one.clone()),
                definition: edited.clone(),
            },
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(Store::update_at(&path, &Mutation::Delete(one)).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let store = Store::update_at(&path, &Mutation::Delete(edited)).unwrap();
        assert_eq!(store.definitions, vec![two]);
        assert!(store.assigned(&worktree).is_none());
        assert!(!worktree.exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn invalid_or_unknown_store_is_never_replaced_and_loading_does_not_write() {
        let path = path();
        assert!(Store::load_at(&path).unwrap().definitions.is_empty());
        assert!(!path.exists());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bytes in [
            b"broken".as_slice(),
            br#"{"version":99,"definitions":[],"assignments":[]}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(
                Store::update_at(
                    &path,
                    &Mutation::Save {
                        previous: None,
                        definition: definition("Work")
                    }
                )
                .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn assignments_preserve_non_utf8_worktree_paths() {
        use std::os::unix::ffi::OsStringExt;
        let path = path();
        let profile = definition("Personal");
        Store::update_at(
            &path,
            &Mutation::Save {
                previous: None,
                definition: profile.clone(),
            },
        )
        .unwrap();
        let worktree = path
            .parent()
            .unwrap()
            .join(std::ffi::OsString::from_vec(b"work-\xff".to_vec()));
        Store::update_at(
            &path,
            &Mutation::Assign {
                worktree: worktree.clone(),
                profile: profile.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            Store::load_at(&path).unwrap().assigned(&worktree),
            Some(&profile)
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
