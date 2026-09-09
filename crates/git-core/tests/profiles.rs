use gitturtle_core::{GitRepository, ProfileIdentity, ProfileSigning, TagCommand, WriteCommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tempfile::TempDir;
struct Fixture {
    temp: TempDir,
    repo: GitRepository,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = GitRepository::init(&temp.path().join("main"), "main").unwrap();
        let result = Self { temp, repo };
        for (key, value) in [
            ("user.name", "Original"),
            ("user.email", "original@example.invalid"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
        ] {
            result.git(&["config", key, value]);
        }
        result
    }
    fn git(&self, args: &[&str]) -> String {
        git(self.repo.path(), args)
    }
    fn apply(&self, profile: ProfileIdentity) {
        let plan = self.repo.profile_plan(profile).unwrap();
        self.repo
            .execute(&WriteCommand::ApplyProfile(Arc::new(plan)))
            .unwrap();
    }
    fn commit(&self, name: &str) -> String {
        fs::write(self.repo.path().join("file"), name).unwrap();
        self.repo.execute(&WriteCommand::StageAll).unwrap();
        self.repo
            .execute(&WriteCommand::Commit {
                message: name.into(),
            })
            .unwrap()
            .commit_oid
            .unwrap()
    }
    fn tag(&self, name: &str) {
        let plan = self
            .repo
            .create_tag_plan(name, "HEAD", Some(format!("Annotated {name}")))
            .unwrap();
        self.repo
            .execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .unwrap();
    }
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn identity(name: &str) -> ProfileIdentity {
    ProfileIdentity {
        name: name.into(),
        email: format!("{}@example.invalid", name.to_lowercase()),
        signing: None,
    }
}
#[test]
fn profiles_change_real_commit_and_annotated_tag_identity_and_keep_other_configuration() {
    let f = Fixture::new();
    f.git(&["config", "custom.keep", "untouched"]);
    let original_signer = f.repo.profile().unwrap().signing_key;
    for name in ["Personal", "Work"] {
        f.apply(identity(name));
        f.commit(name);
        f.tag(name);
        assert_eq!(
            f.git(&["show", "-s", "--format=%an <%ae>|%cn <%ce>", "HEAD"]),
            format!(
                "{name} <{}@example.invalid>|{name} <{}@example.invalid>",
                name.to_lowercase(),
                name.to_lowercase()
            )
        );
        assert!(f.git(&["cat-file", "tag", name]).contains(&format!(
            "tagger {name} <{}@example.invalid>",
            name.to_lowercase()
        )));
    }
    assert_eq!(f.git(&["config", "custom.keep"]), "untouched");
    assert_eq!(f.repo.profile().unwrap().signing_key, original_signer);
    assert_eq!(f.git(&["config", "core.hooksPath"]), ".git/hooks");
}
#[test]
fn shared_and_existing_private_worktree_configuration_have_distinct_scope() {
    let f = Fixture::new();
    f.commit("initial");
    let linked = f.temp.path().join("linked");
    f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    let repo = GitRepository::open(&linked).unwrap();
    let plan = repo.profile_plan(identity("Shared")).unwrap();
    assert!(!plan.private_worktree);
    repo.execute(&WriteCommand::ApplyProfile(Arc::new(plan)))
        .unwrap();
    assert_eq!(f.repo.profile().unwrap().name, "Shared");
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    let plan = repo.profile_plan(identity("Private")).unwrap();
    assert!(plan.private_worktree);
    let config = plan.config_path.clone();
    repo.execute(&WriteCommand::ApplyProfile(Arc::new(plan)))
        .unwrap();
    assert!(config.ends_with("config.worktree"));
    assert_eq!(repo.profile().unwrap().name, "Private");
    assert_eq!(f.repo.profile().unwrap().name, "Shared");
}
#[test]
fn stale_plan_existing_lock_and_invalid_fields_never_partially_change_identity() {
    let f = Fixture::new();
    let plan = f.repo.profile_plan(identity("Work")).unwrap();
    f.git(&["config", "custom.concurrent", "retain"]);
    let config = f.repo.path().join(".git/config");
    let bytes = fs::read(&config).unwrap();
    assert!(
        f.repo
            .execute(&WriteCommand::ApplyProfile(Arc::new(plan)))
            .is_err()
    );
    assert_eq!(fs::read(&config).unwrap(), bytes);
    let plan = f.repo.profile_plan(identity("Work")).unwrap();
    fs::write(config.with_extension("lock"), b"someone else's lock").unwrap();
    assert!(
        f.repo
            .execute(&WriteCommand::ApplyProfile(Arc::new(plan)))
            .is_err()
    );
    assert_eq!(fs::read(&config).unwrap(), bytes);
    assert_eq!(
        fs::read(config.with_extension("lock")).unwrap(),
        b"someone else's lock"
    );
    let mut invalid = identity("Work");
    invalid.name = "new\n[user]".into();
    assert!(f.repo.profile_plan(invalid).is_err());
    let mut invalid = identity("Work");
    invalid.signing = Some(ProfileSigning {
        key: Some("-----BEGIN PRIVATE KEY-----".into()),
        ..Default::default()
    });
    assert!(f.repo.profile_plan(invalid).is_err());
    assert_eq!(fs::read(&config).unwrap(), bytes);
}
#[test]
fn included_identity_is_preserved_on_disk_but_explicit_profile_wins_and_signing_stays_required() {
    let f = Fixture::new();
    let include = f.temp.path().join("included-config");
    let original =
        b"[user]\n name = Included\n email = included@example.invalid\n[commit]\n gpgsign = true\n";
    fs::write(&include, original).unwrap();
    f.git(&["config", "include.path", include.to_str().unwrap()]);
    assert_eq!(f.repo.profile().unwrap().name, "Included");
    f.apply(identity("Work"));
    let effective = f.repo.profile().unwrap();
    assert_eq!(effective.name, "Work");
    assert!(effective.signing);
    assert_eq!(fs::read(include).unwrap(), original);
    let mut profile = identity("Personal");
    profile.signing = Some(ProfileSigning::default());
    f.apply(profile);
    assert!(f.repo.profile().unwrap().signing);
}
#[cfg(unix)]
#[test]
fn two_ssh_signing_profiles_sign_commits_and_tags_and_missing_key_never_falls_back() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let hook = f.repo.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nprintf hook-ran > hook-evidence\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    let mut keys: Vec<PathBuf> = Vec::new();
    let mut allowed = String::new();
    for name in ["Personal", "Work"] {
        let key = f.temp.path().join(name);
        let output = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .output()
            .unwrap();
        assert!(output.status.success());
        allowed.push_str(&format!(
            "{}@example.invalid {}\n",
            name.to_lowercase(),
            fs::read_to_string(key.with_extension("pub"))
                .unwrap()
                .trim()
        ));
        keys.push(key);
    }
    let signers = f.temp.path().join("allowed-signers");
    fs::write(&signers, allowed).unwrap();
    f.git(&[
        "config",
        "gpg.ssh.allowedSignersFile",
        signers.to_str().unwrap(),
    ]);
    for (name, key) in ["Personal", "Work"].into_iter().zip(&keys) {
        let mut profile = identity(name);
        profile.signing = Some(ProfileSigning {
            key: Some(key.to_string_lossy().into_owned()),
            format: Some("ssh".into()),
            commits: true,
            tags: true,
        });
        f.apply(profile);
        assert!(
            f.repo.profile().unwrap().tag_signing,
            "{}",
            fs::read_to_string(f.repo.path().join(".git/config")).unwrap()
        );
        assert_eq!(f.git(&["config", "--bool", "--get", "tag.gpgSign"]), "true");
        f.commit(name);
        f.tag(name);
        f.git(&["verify-commit", "HEAD"]);
        assert!(
            f.git(&["cat-file", "tag", name])
                .contains("\n-----BEGIN SSH SIGNATURE-----"),
            "Signed annotation lacks a signature line boundary: {}",
            f.git(&["cat-file", "tag", name])
        );
        f.git(&["verify-tag", name]);
        assert_eq!(
            fs::read_to_string(f.repo.path().join("hook-evidence")).unwrap(),
            "hook-ran"
        );
    }
    let head = f.git(&["rev-parse", "HEAD"]);
    fs::remove_file(&keys[1]).unwrap();
    fs::write(f.repo.path().join("file"), "unsigned must fail").unwrap();
    f.repo.execute(&WriteCommand::StageAll).unwrap();
    let index = f.git(&["ls-files", "--stage"]);
    assert!(
        f.repo
            .execute(&WriteCommand::Commit {
                message: "must fail".into()
            })
            .is_err()
    );
    let plan = f
        .repo
        .create_tag_plan("must-fail", "HEAD", Some("must fail".into()))
        .unwrap();
    assert!(
        f.repo
            .execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .is_err()
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["ls-files", "--stage"]), index);
    assert_eq!(
        f.repo.profile().unwrap().signing_key.as_deref(),
        keys[1].to_str()
    );
    assert!(f.repo.profile().unwrap().signing);
}
