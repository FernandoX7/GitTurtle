use gitturtle_core::{GitRepository, IgnoreDestination, TagCommand, WriteCommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        GitRepository::init(&root, "main").unwrap();
        let f = Self { _temp: temp, root };
        f.git(&["config", "user.name", "Tag Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgSign", "false"]);
        f.git(&["config", "tag.gpgSign", "false"]);
        f.write("tracked", "base\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-m", "Initial"]);
        f
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim_end().to_owned()
    }
}

#[test]
fn tags_capture_commits_preserve_work_and_delete_only_reviewed_oid() {
    let f = Fixture::new();
    let repo = f.repo();
    let original = f.git(&["rev-parse", "HEAD"]);
    let plan = repo.create_tag_plan("v1", "HEAD", None).unwrap();
    f.write("tracked", "second\n");
    f.git(&["commit", "-am", "Second"]);
    f.write("tracked", "independent unstaged\n");
    f.write("staged", "staged\n");
    f.git(&["add", "staged"]);
    let index = f.git(&["write-tree"]);
    repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
        .unwrap();
    assert_eq!(f.git(&["rev-parse", "v1"]), original);
    assert_eq!(f.git(&["write-tree"]), index);
    assert_eq!(
        fs::read(f.root.join("tracked")).unwrap(),
        b"independent unstaged\n"
    );
    let selected = repo.tags().unwrap().tags.pop().unwrap();
    f.git(&["tag", "--force", "v1", "HEAD"]);
    assert!(
        repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Delete(selected))))
            .is_err()
    );
    assert_eq!(f.git(&["rev-parse", "v1"]), f.git(&["rev-parse", "HEAD"]));
    let selected = repo.tags().unwrap().tags.pop().unwrap();
    repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Delete(selected))))
        .unwrap();
    assert!(repo.tags().unwrap().tags.is_empty());
}

#[test]
fn annotation_inspection_preserves_message_and_never_bypasses_signing() {
    let f = Fixture::new();
    let repo = f.repo();
    let plan = repo
        .create_tag_plan("release/test", "HEAD", Some("Title\n\n  body  \n".into()))
        .unwrap();
    repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
        .unwrap();
    let tag = repo.tags().unwrap().tags.pop().unwrap();
    assert!(tag.annotated);
    assert_eq!(tag.target_kind, "commit");
    assert_eq!(
        repo.tag_details(&tag).unwrap().message,
        "Title\n\n  body  \n"
    );
    assert_eq!(tag.tagger, "Tag Fixture");
    f.git(&["config", "tag.gpgSign", "true"]);
    f.git(&["config", "gpg.program", "/usr/bin/false"]);
    assert!(repo.create_tag_plan("light", "HEAD", None).is_err());
    let plan = repo
        .create_tag_plan("signed", "HEAD", Some("Signing must fail".into()))
        .unwrap();
    assert!(plan.signing);
    assert!(
        repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .is_err()
    );
    assert_eq!(repo.tags().unwrap().tags.len(), 1);
}

#[test]
fn named_push_ignores_mirror_and_follow_tags_configuration() {
    let f = Fixture::new();
    let remote = f.root.parent().unwrap().join("remote.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .unwrap()
            .status
            .success()
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    f.git(&["tag", "-a", "only", "-m", "one"]);
    f.git(&["tag", "-a", "private", "-m", "two"]);
    f.git(&["config", "remote.origin.mirror", "true"]);
    f.git(&["config", "push.followTags", "true"]);
    let repo = f.repo();
    let tag = repo
        .tags()
        .unwrap()
        .tags
        .into_iter()
        .find(|tag| tag.name == "only")
        .unwrap();
    let remote_config = repo.remote_configs().unwrap().pop().unwrap();
    repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Push {
        tag,
        remote: Arc::new(remote_config),
    })))
    .unwrap();
    let refs = Command::new("git")
        .arg("-C")
        .arg(remote)
        .args(["for-each-ref", "--format=%(refname)"])
        .output()
        .unwrap();
    assert_eq!(refs.stdout, b"refs/tags/only\n");
}

#[test]
fn literal_ignore_preserves_crlf_and_unrelated_index_and_work() {
    let f = Fixture::new();
    let name = "folder/[a]*? !#\\ ";
    f.write(name, "ignore me");
    f.write("folder/aZZ", "keep me");
    f.write(".gitignore", "# existing\r\nold");
    f.write("tracked", "staged");
    f.git(&["add", "tracked"]);
    f.write("tracked", "unstaged");
    let repo = f.repo();
    let index = f.git(&["write-tree"]);
    let plan = repo
        .ignore_plan(Path::new(name), false, IgnoreDestination::Shared)
        .unwrap();
    assert_eq!(plan.rule, br"/folder/\[a\]\*\?\ \!\#\\\ ");
    repo.execute(&WriteCommand::Ignore(Arc::new(plan))).unwrap();
    let actual = fs::read(f.root.join(".gitignore")).unwrap();
    assert!(actual.starts_with(b"# existing\r\nold\r\n"));
    assert!(actual.ends_with(b"\r\n"));
    assert!(
        !repo
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.path == Path::new(name))
    );
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.path == Path::new("folder/aZZ") && entry.untracked)
    );
    assert_eq!(f.git(&["write-tree"]), index);
    assert_eq!(fs::read(f.root.join("tracked")).unwrap(), b"unstaged");
}

#[test]
fn ignore_rejects_stale_content_newly_tracked_target_and_symlinks() {
    let f = Fixture::new();
    f.write("new", "new");
    f.write(".gitignore", "# before\n");
    let repo = f.repo();
    let plan = repo
        .ignore_plan(Path::new("new"), false, IgnoreDestination::Shared)
        .unwrap();
    f.write(".gitignore", "# external edit\n");
    assert!(repo.execute(&WriteCommand::Ignore(Arc::new(plan))).is_err());
    assert_eq!(
        fs::read(f.root.join(".gitignore")).unwrap(),
        b"# external edit\n"
    );
    let plan = repo
        .ignore_plan(Path::new("new"), false, IgnoreDestination::Shared)
        .unwrap();
    f.git(&["add", "new"]);
    assert!(repo.execute(&WriteCommand::Ignore(Arc::new(plan))).is_err());
    #[cfg(unix)]
    {
        f.write("another", "new");
        let outside = f.root.parent().unwrap().join("outside");
        fs::write(&outside, b"precious").unwrap();
        fs::remove_file(f.root.join(".gitignore")).unwrap();
        std::os::unix::fs::symlink(&outside, f.root.join(".gitignore")).unwrap();
        assert!(
            repo.ignore_plan(Path::new("another"), false, IgnoreDestination::Shared)
                .is_err()
        );
        assert_eq!(fs::read(outside).unwrap(), b"precious");
    }
}

#[test]
fn directory_local_exclude_preserves_tracked_paths_and_shared_rules() {
    let f = Fixture::new();
    f.write("cache/tracked", "retained");
    f.git(&["add", "cache/tracked"]);
    f.git(&["commit", "-m", "Tracked cache"]);
    f.write("cache/new", "temporary");
    f.write("cache/more", "temporary");
    let repo = f.repo();
    let index = f.git(&["write-tree"]);
    let plan = repo
        .ignore_plan(Path::new("cache/new"), true, IgnoreDestination::Local)
        .unwrap();
    assert_eq!(plan.rule, b"/cache/");
    assert_eq!(plan.tracked_paths, 1);
    repo.execute(&WriteCommand::Ignore(Arc::new(plan))).unwrap();
    assert!(!f.root.join(".gitignore").exists());
    assert_eq!(f.git(&["write-tree"]), index);
    assert_eq!(f.git(&["ls-files", "cache"]), "cache/tracked");
    assert!(repo.status().unwrap().entries.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn ignore_handles_non_utf8_paths_and_refuses_newlines() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    let path = PathBuf::from(std::ffi::OsString::from_vec(b"bad-\xff*".to_vec()));
    f.write(&path, "bytes");
    f.write("line\nbreak", "bytes");
    let repo = f.repo();
    let plan = repo
        .ignore_plan(&path, false, IgnoreDestination::Shared)
        .unwrap();
    assert!(!plan.rule_is_utf8());
    repo.execute(&WriteCommand::Ignore(Arc::new(plan))).unwrap();
    assert!(
        !repo
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.path == path)
    );
    assert!(
        repo.ignore_plan(Path::new("line\nbreak"), false, IgnoreDestination::Shared)
            .is_err()
    );
}

#[test]
fn ignore_rejects_line_breaks_without_touching_destination() {
    let f = Fixture::new();
    f.write("line\nbreak", "bytes");
    assert!(
        f.repo()
            .ignore_plan(Path::new("line\nbreak"), false, IgnoreDestination::Shared)
            .is_err()
    );
    assert!(!f.root.join(".gitignore").exists());
}

#[cfg(unix)]
#[test]
fn ignore_refuses_replaced_file_identity_and_symbolic_exclude_parent() {
    let f = Fixture::new();
    f.write("new", "bytes");
    f.write(".gitignore", "# existing\n");
    let repo = f.repo();
    let plan = repo
        .ignore_plan(Path::new("new"), false, IgnoreDestination::Shared)
        .unwrap();
    f.write("replacement", "# existing\n");
    fs::rename(f.root.join("replacement"), f.root.join(".gitignore")).unwrap();
    assert!(repo.execute(&WriteCommand::Ignore(Arc::new(plan))).is_err());
    let plan = repo
        .ignore_plan(Path::new("new"), false, IgnoreDestination::Local)
        .unwrap();
    let info = f.root.join(".git/info");
    let retained = f.root.join(".git/retained-info");
    fs::rename(&info, &retained).unwrap();
    let outside = f.root.parent().unwrap().join("outside-info");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("exclude"), b"precious\n").unwrap();
    std::os::unix::fs::symlink(&outside, &info).unwrap();
    assert!(repo.execute(&WriteCommand::Ignore(Arc::new(plan))).is_err());
    assert_eq!(fs::read(outside.join("exclude")).unwrap(), b"precious\n");
}

#[test]
fn captured_tag_push_refuses_changed_remote_configuration() {
    let f = Fixture::new();
    let first = f.root.parent().unwrap().join("first.git");
    let second = f.root.parent().unwrap().join("second.git");
    for remote in [&first, &second] {
        assert!(
            Command::new("git")
                .args(["init", "--bare"])
                .arg(remote)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    f.git(&["remote", "add", "origin", first.to_str().unwrap()]);
    f.git(&["tag", "local"]);
    let repo = f.repo();
    let tag = repo.tags().unwrap().tags.pop().unwrap();
    let remote = repo.remote_configs().unwrap().pop().unwrap();
    f.git(&["remote", "set-url", "origin", second.to_str().unwrap()]);
    assert!(
        repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Push {
            tag,
            remote: Arc::new(remote)
        })))
        .is_err()
    );
    for remote in [&first, &second] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(remote)
                .args(["for-each-ref"])
                .output()
                .unwrap()
                .stdout
                .is_empty()
        );
    }
}

#[test]
fn tag_creation_refuses_signing_configuration_changed_since_review() {
    let f = Fixture::new();
    let repo = f.repo();
    let plan = repo
        .create_tag_plan("reviewed", "HEAD", Some("Reviewed unsigned".into()))
        .unwrap();
    f.git(&["config", "tag.gpgSign", "true"]);
    assert!(
        repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .is_err()
    );
    assert!(repo.tags().unwrap().tags.is_empty());
}

#[test]
fn oversized_annotation_keeps_inspection_identity_and_safe_delete_available() {
    let f = Fixture::new();
    f.write("tag-message", vec![b'x'; 300 * 1024]);
    f.git(&["tag", "--annotate", "large", "--file=tag-message"]);
    let repo = f.repo();
    let tag = repo.tags().unwrap().tags.pop().unwrap();
    let details = repo.tag_details(&tag).unwrap();
    assert!(details.annotation_unavailable.is_some());
    assert!(details.message.is_empty());
    assert_eq!(details.tag.oid, tag.oid);
    repo.execute(&WriteCommand::Tag(Arc::new(TagCommand::Delete(tag))))
        .unwrap();
    assert!(repo.tags().unwrap().tags.is_empty());
    assert_eq!(
        fs::read(f.root.join("tag-message")).unwrap().len(),
        300 * 1024
    );
}
