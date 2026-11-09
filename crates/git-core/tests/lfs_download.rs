use gitturtle_core::{
    ChangeStatus, FileChange, GitRepository, LfsDownloadTarget, LfsObjectFailure, OperationControl,
    TextPreview, WriteCommand, run_controlled,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    remote: PathBuf,
    pointer: Vec<u8>,
    payload: Vec<u8>,
    oid: String,
    blob: String,
}
impl Fixture {
    fn new() -> Option<Self> {
        // Downloads refuse releases before 3.8.1, which
        // `git_lfs_before_3_8_1_is_refused_before_any_transfer` covers instead.
        if installed_lfs()? < (3, 8, 1) {
            eprintln!(
                "Git LFS older than 3.8.1 is refused for downloads; local transfer fixture not run"
            );
            return None;
        }
        Some(Self::create())
    }
    fn create() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        let remote = temp.path().join("remote.git");
        GitRepository::init(&root, "main").unwrap();
        let root = GitRepository::open(&root).unwrap().path().to_owned();
        assert!(
            Command::new("git")
                .args(["init", "--bare", "--quiet"])
                .arg(&remote)
                .status()
                .unwrap()
                .success()
        );
        git(&root, &["config", "user.name", "LFS Fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        git(&root, &["config", "commit.gpgSign", "false"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        let payload = b"one exact LFS preview payload\n".to_vec();
        let oid = format!("{:x}", Sha256::digest(&payload));
        let pointer = pointer(&oid, payload.len());
        fs::write(root.join("image.png"), &pointer).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "Pointer"]);
        let blob = git(&root, &["rev-parse", "HEAD:image.png"]);
        let storage = object(&remote.join("lfs"), &oid);
        fs::create_dir_all(storage.parent().unwrap()).unwrap();
        fs::write(storage, &payload).unwrap();
        Self {
            _temp: temp,
            root,
            remote,
            pointer,
            payload,
            oid,
            blob,
        }
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn target(&self) -> LfsDownloadTarget {
        LfsDownloadTarget {
            path: "image.png".into(),
            blob_oid: Some(self.blob.clone()),
            pointer: self.pointer.clone(),
        }
    }
}
fn pointer(oid: &str, size: usize) -> Vec<u8> {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
        .into_bytes()
}
fn object(storage: &Path, oid: &str) -> PathBuf {
    storage
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid)
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().into()
}
/// The installed `git-lfs` release, or None when it is unavailable.
fn installed_lfs() -> Option<(u64, u64, u64)> {
    let version = Command::new("git").args(["lfs", "version"]).output().ok();
    let release = version
        .filter(|version| version.status.success())
        .and_then(|version| {
            let output = String::from_utf8_lossy(&version.stdout).into_owned();
            let parts = output
                .split_whitespace()
                .next()?
                .strip_prefix("git-lfs/")?
                .split('.')
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            Some((*parts.first()?, *parts.get(1)?, *parts.get(2)?))
        });
    if release.is_none() {
        eprintln!("Git LFS unavailable; local transfer fixture not run");
    }
    release
}

/// Each side of a text comparison resolves on its own from the local store:
/// one missing object no longer turns the other side back into its pointer.
/// Where no side resolves, or the one that does has no text, the two pointers
/// stay, and a failure beside a missing side stays what it was.
#[test]
fn each_lfs_text_side_resolves_on_its_own_and_reads_only_the_local_store() {
    let fixture = Fixture::create();
    let repo = fixture.repo();
    let store = fixture.root.join(".git/lfs");
    let local = |bytes: &[u8]| {
        let oid = format!("{:x}", Sha256::digest(bytes));
        let path = object(&store, &oid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        pointer(&oid, bytes.len())
    };
    let (before, after) = (b"before\n".as_slice(), b"after\n".as_slice());
    let (old_local, new_local) = (local(before), local(after));
    let binary = local(b"\0binary\n");
    let large = local("x\n".repeat(1_100_000).as_bytes());
    let corrupt = local(b"corrupt\n");
    fs::write(
        object(&store, &format!("{:x}", Sha256::digest(b"corrupt\n"))),
        b"tampered",
    )
    .unwrap();
    let missing = pointer(&"c".repeat(64), 42);
    let missing_too = pointer(&"d".repeat(64), 43);
    let oversize = pointer(&"e".repeat(64), 33 * 1024 * 1024);
    let file = FileChange {
        old_path: Some("notes.txt".into()),
        new_path: Some("notes.txt".into()),
        old_oid: None,
        new_oid: None,
        status: ChangeStatus::Modified,
        old_mode: "100644".into(),
        new_mode: "100644".into(),
    };
    let listing = || {
        fn walk(path: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, found);
                }
                found.push(path);
            }
        }
        let mut found = Vec::new();
        walk(&store, &mut found);
        found.sort();
        found
    };
    let held = listing();
    let resolve = |old: &[u8], new: &[u8]| repo.resolved_lfs_text(&file, old, new);

    // Both local, as ever.
    let both = resolve(&old_local, &new_local).unwrap();
    assert_eq!(both.missing, [false, false]);
    assert_eq!(both.pointers, [true, true], "as stored, not as resolved");
    assert_eq!(
        resolve(before, &missing).unwrap().pointers,
        [false, true],
        "plain text is no pointer"
    );
    let both = both.text.unwrap();
    assert_eq!((both.old.as_slice(), both.new.as_slice()), (before, after));
    assert!(matches!(both.preview, TextPreview::Patch(_)));

    // One missing: the other is still resolved.
    for (name, old, new, shown_old, shown_new, absent) in [
        (
            "only the new object missing",
            old_local.as_slice(),
            missing.as_slice(),
            before,
            missing.as_slice(),
            [false, true],
        ),
        (
            "only the old object missing",
            missing.as_slice(),
            new_local.as_slice(),
            missing.as_slice(),
            after,
            [true, false],
        ),
    ] {
        let outcome = resolve(old, new).unwrap();
        assert_eq!(outcome.missing, absent, "{name}");
        let resolved = outcome
            .text
            .unwrap_or_else(|| panic!("{name}: the other side resolves"));
        assert_eq!(resolved.old, shown_old, "{name}");
        assert_eq!(resolved.new, shown_new, "{name}");
        assert!(matches!(resolved.preview, TextPreview::Patch(_)), "{name}");
    }

    // Nothing resolved keeps the two pointers, and says which are truly
    // absent: a local object that cannot be shown is not one of them.
    for (name, old, new, absent) in [
        (
            "both missing",
            missing.as_slice(),
            missing_too.as_slice(),
            [true, true],
        ),
        ("no pointer on either side", before, after, [false, false]),
        (
            "a missing object and plain text",
            missing.as_slice(),
            after,
            [true, false],
        ),
        (
            "plain text and a missing object",
            before,
            missing.as_slice(),
            [false, true],
        ),
        // A resolved side with no text has nothing to set beside a pointer.
        (
            "binary content beside a missing object",
            binary.as_slice(),
            missing.as_slice(),
            [false, true],
        ),
        (
            "a missing object beside binary content",
            missing.as_slice(),
            binary.as_slice(),
            [true, false],
        ),
        (
            "a missing object beside text past the preview limit",
            missing.as_slice(),
            large.as_slice(),
            [true, false],
        ),
        // What a missing side once left the pointers for, it still does.
        (
            "a missing object before a corrupt one",
            missing.as_slice(),
            corrupt.as_slice(),
            [true, false],
        ),
        (
            "a missing object before one past the limit",
            missing.as_slice(),
            oversize.as_slice(),
            [true, false],
        ),
    ] {
        let outcome = resolve(old, new).unwrap();
        assert!(outcome.text.is_none(), "{name}");
        assert_eq!(outcome.missing, absent, "{name}");
    }

    // Both resolved, a binary side keeps its explicit state.
    let resolved = resolve(&binary, &new_local).unwrap().text.unwrap();
    assert!(matches!(resolved.preview, TextPreview::Binary));

    // One side's object failing is that side's alone: it stays its pointer,
    // is never called missing, and the other side keeps its preview.
    let (corrupt, oversize, missing_side) =
        (corrupt.as_slice(), oversize.as_slice(), missing.as_slice());
    let (old_side, new_side) = (old_local.as_slice(), new_local.as_slice());
    use LfsObjectFailure::{Corrupt, TooLarge};
    for (name, old, new, shown, absent, failed) in [
        (
            "corrupt first beside a missing object",
            corrupt,
            missing_side,
            None,
            [false, true],
            [Some(Corrupt), None],
        ),
        (
            "past the limit first beside a missing object",
            oversize,
            missing_side,
            None,
            [false, true],
            [Some(TooLarge), None],
        ),
        (
            "corrupt second beside a local object",
            old_side,
            corrupt,
            Some((before, corrupt)),
            [false, false],
            [None, Some(Corrupt)],
        ),
        (
            "corrupt first beside a local object",
            corrupt,
            new_side,
            Some((corrupt, after)),
            [false, false],
            [Some(Corrupt), None],
        ),
        (
            "past the limit first beside a local object",
            oversize,
            new_side,
            Some((oversize, after)),
            [false, false],
            [Some(TooLarge), None],
        ),
        (
            "corrupt first beside plain text",
            corrupt,
            after,
            None,
            [false, false],
            [Some(Corrupt), None],
        ),
        (
            "plain text first beside past the limit",
            before,
            oversize,
            None,
            [false, false],
            [None, Some(TooLarge)],
        ),
        (
            "both failing",
            corrupt,
            oversize,
            None,
            [false, false],
            [Some(Corrupt), Some(TooLarge)],
        ),
    ] {
        let outcome = resolve(old, new).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        assert_eq!(outcome.missing, absent, "{name}");
        assert_eq!(outcome.failed, failed, "{name}");
        match (outcome.text, shown) {
            (Some(text), Some((shown_old, shown_new))) => {
                assert_eq!(text.old, shown_old, "{name}");
                assert_eq!(text.new, shown_new, "{name}");
                assert!(matches!(text.preview, TextPreview::Patch(_)), "{name}");
            }
            (None, None) => {}
            (text, shown) => panic!("{name}: resolved {:?}, expected {shown:?}", text.is_some()),
        }
    }

    assert_eq!(listing(), held, "resolving writes nothing to the store");
}

/// A store that cannot be read is no statement about the object: a link in
/// the store's place or in its layout, or a read the system refuses, is the
/// store's failure, and only what the read found wrong with the object's own
/// bytes calls the object corrupt. Either way the side stays its pointer and
/// is not called missing, so nothing offers a download.
#[cfg(unix)]
#[test]
fn a_store_that_cannot_be_read_is_not_called_a_corrupt_object() {
    use LfsObjectFailure::{Corrupt, StoreUnreadable};
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::create();
    let repo = fixture.repo();
    let store = fixture.root.join(".git/lfs");
    let bytes = b"the object's bytes\n";
    let oid = format!("{:x}", Sha256::digest(bytes));
    let stored = pointer(&oid, bytes.len());
    let file = FileChange {
        old_path: Some("notes.txt".into()),
        new_path: Some("notes.txt".into()),
        old_oid: None,
        new_oid: None,
        status: ChangeStatus::Modified,
        old_mode: "100644".into(),
        new_mode: "100644".into(),
    };
    let plain = b"plain text\n";
    let failed = |name: &str, expected: LfsObjectFailure| {
        let outcome = repo
            .resolved_lfs_text(&file, &stored, plain)
            .unwrap_or_else(|error| panic!("{name}: the preview failed: {error:#}"));
        assert_eq!(outcome.failed, [Some(expected), None], "{name}");
        assert_eq!(outcome.missing, [false, false], "{name}");
    };
    let path = object(&store, &oid);
    let leaf = path.parent().unwrap().to_owned();
    // A good copy of the object, in a directory a link can lead to.
    let hold = |directory: &Path| {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join(&oid), bytes).unwrap();
    };

    // A link in the store's place, though the object it leads to is good.
    let linked_store = fixture.root.join(".git/linked-store");
    hold(object(&linked_store, &oid).parent().unwrap());
    symlink(&linked_store, &store).unwrap();
    failed("a linked store", StoreUnreadable);
    fs::remove_file(&store).unwrap();

    // A link in one of the store's directories.
    let elsewhere = fixture.root.join(".git/elsewhere");
    hold(&elsewhere);
    fs::create_dir_all(leaf.parent().unwrap()).unwrap();
    symlink(&elsewhere, &leaf).unwrap();
    failed("a linked directory", StoreUnreadable);
    fs::remove_file(&leaf).unwrap();

    // A link in the object's place.
    fs::create_dir_all(&leaf).unwrap();
    symlink(elsewhere.join(&oid), &path).unwrap();
    failed("a linked object", StoreUnreadable);
    fs::remove_file(&path).unwrap();

    // What the read found wrong with the object itself stays corrupt.
    fs::write(&path, b"short").unwrap();
    failed("the wrong size", Corrupt);
    fs::write(&path, vec![b'x'; bytes.len()]).unwrap();
    failed("the wrong digest", Corrupt);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    failed("a directory in the object's place", Corrupt);
    fs::remove_dir(&path).unwrap();

    // Permissions, where they bind: a superuser's read is never refused.
    fs::write(&path, bytes).unwrap();
    let chmod = |target: &Path, bits: u32| {
        fs::set_permissions(target, fs::Permissions::from_mode(bits)).unwrap();
    };
    let objects = store.join("objects");
    chmod(&path, 0o000);
    let bound = fs::File::open(&path).is_err();
    if bound {
        failed("an object the read is refused", StoreUnreadable);
    }
    chmod(&path, 0o644);
    chmod(&objects, 0o000);
    if bound {
        failed("a directory the read is refused", StoreUnreadable);
    }
    chmod(&objects, 0o755);

    // Repaired, the same pointer resolves: the failure was the store's.
    let outcome = repo.resolved_lfs_text(&file, &stored, plain).unwrap();
    assert_eq!(outcome.failed, [None, None]);
    assert_eq!(outcome.text.expect("it resolves").old, bytes);
}

#[test]
fn git_lfs_before_3_8_1_is_refused_before_any_transfer() {
    let Some(release) = installed_lfs() else {
        return;
    };
    if release >= (3, 8, 1) {
        eprintln!("Git LFS {release:?} is patched; refusal fixture not run");
        return;
    }
    let fixture = Fixture::create();
    let error = fixture
        .repo()
        .lfs_download_plan(&fixture.target(), "origin")
        .unwrap_err()
        .to_string();
    assert!(error.contains("CVE-2026-105803"), "{error}");
    assert!(
        error.contains("Update Git LFS to 3.8.1 or newer"),
        "{error}"
    );
    // No Git LFS command ran against the repository, so nothing created its storage.
    assert!(!fixture.root.join(".git/lfs").exists());
}

#[test]
fn lfs_preparation_is_passive_and_download_is_exact_verified_and_preserves_work() {
    let Some(f) = Fixture::new() else {
        return;
    };
    let repo = f.repo();
    let other = b"another object that must stay remote\n";
    let other_oid = format!("{:x}", Sha256::digest(other));
    fs::write(f.root.join("other.png"), pointer(&other_oid, other.len())).unwrap();
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-m", "Other pointer"]);
    let remote_other = object(&f.remote.join("lfs"), &other_oid);
    fs::create_dir_all(remote_other.parent().unwrap()).unwrap();
    fs::write(remote_other, other).unwrap();
    git(&f.root, &["config", "lfs.fetchrecentalways", "true"]);
    fs::write(f.root.join("unrelated"), "staged\n").unwrap();
    git(&f.root, &["add", "unrelated"]);
    fs::write(f.root.join("unrelated"), "unstaged\n").unwrap();
    let head = git(&f.root, &["rev-parse", "HEAD"]);
    let index = git(&f.root, &["write-tree"]);
    let plan = repo.lfs_download_plan(&f.target(), "origin").unwrap();
    assert!(
        !f.root.join(".git/lfs").exists(),
        "LFS config preparation must not create repository storage"
    );
    assert_eq!(plan.oid, f.oid);
    assert_eq!(plan.size, f.payload.len() as u64);
    assert!(plan.source.contains("remote.git"));
    repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
        .unwrap();
    assert_eq!(
        repo.local_lfs_object(&f.oid, f.payload.len() as u64, 1024)
            .unwrap()
            .unwrap(),
        f.payload
    );
    assert!(!object(&f.root.join(".git/lfs"), &other_oid).exists());
    assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&f.root, &["write-tree"]), index);
    assert_eq!(fs::read(f.root.join("image.png")).unwrap(), f.pointer);
    assert_eq!(fs::read(f.root.join("unrelated")).unwrap(), b"unstaged\n");
}

#[test]
fn raw_working_pointer_fetch_uses_private_temporary_git_objects() {
    let Some(f) = Fixture::new() else {
        return;
    };
    let repo = f.repo();
    let payload = b"not present in any Git pointer blob\n";
    let oid = format!("{:x}", Sha256::digest(payload));
    let bytes = pointer(&oid, payload.len());
    let remote = object(&f.remote.join("lfs"), &oid);
    fs::create_dir_all(remote.parent().unwrap()).unwrap();
    fs::write(remote, payload).unwrap();
    fs::write(f.root.join("image.png"), &bytes).unwrap();
    let target = LfsDownloadTarget {
        path: "image.png".into(),
        blob_oid: None,
        pointer: bytes.clone(),
    };
    let plan = repo.lfs_download_plan(&target, "origin").unwrap();
    let before = git(&f.root, &["count-objects", "-v"]);
    repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
        .unwrap();
    assert_eq!(git(&f.root, &["count-objects", "-v"]), before);
    assert_eq!(
        repo.local_lfs_object(&oid, payload.len() as u64, 1024)
            .unwrap()
            .unwrap(),
        payload
    );
    assert_eq!(fs::read(f.root.join("image.png")).unwrap(), bytes);
}

#[test]
fn stale_sources_changed_working_pointers_and_bad_remote_objects_are_refused() {
    let Some(f) = Fixture::new() else {
        return;
    };
    let repo = f.repo();
    let plan = repo.lfs_download_plan(&f.target(), "origin").unwrap();
    git(
        &f.root,
        &["config", "lfs.url", "https://example.invalid/changed-lfs"],
    );
    assert!(
        repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
            .is_err()
    );
    assert!(!f.root.join(".git/lfs").exists());
    git(&f.root, &["config", "--unset", "lfs.url"]);
    let mut target = f.target();
    target.blob_oid = None;
    let plan = repo.lfs_download_plan(&target, "origin").unwrap();
    fs::write(f.root.join("image.png"), "changed\n").unwrap();
    assert!(
        repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
            .is_err()
    );
    let plan = repo.lfs_download_plan(&f.target(), "origin").unwrap();
    fs::write(
        object(&f.remote.join("lfs"), &f.oid),
        b"corrupt remote bytes",
    )
    .unwrap();
    assert!(
        repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
            .is_err()
    );
    assert!(
        repo.local_lfs_object(&f.oid, f.payload.len() as u64, 1024)
            .unwrap()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn cancelling_an_active_lfs_transfer_stops_it_without_touching_index_or_working_files() {
    use std::os::unix::fs::PermissionsExt;
    let Some(f) = Fixture::new() else {
        return;
    };
    let repo = f.repo();
    let script = f.root.parent().unwrap().join("slow-transfer");
    let marker = f.root.parent().unwrap().join("transfer-started");
    // The fixture adapter reports initialization, then holds an actual requested
    // download open until OperationControl terminates the process group.
    let mut file = fs::File::create(&script).unwrap();
    write!(file, "#!/bin/sh\nread request\nprintf '{{}}\\n'\nread request\nprintf started > '{}'\nsleep 10\n", marker.display()).unwrap();
    drop(file);
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    git(
        &f.root,
        &["config", "lfs.standalonetransferagent", "fixture"],
    );
    git(
        &f.root,
        &[
            "config",
            "lfs.customtransfer.fixture.path",
            script.to_str().unwrap(),
        ],
    );
    let plan = repo.lfs_download_plan(&f.target(), "origin").unwrap();
    assert!(!marker.exists());
    let index = git(&f.root, &["write-tree"]);
    let control = OperationControl::default();
    let active = control.clone();
    let task = thread::spawn(move || {
        run_controlled(active, || {
            repo.execute(&WriteCommand::DownloadLfs(Arc::new(plan)))
        })
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(marker.exists(), "Fixture never entered transfer");
    let cancelled = Instant::now();
    control.cancel();
    assert!(
        task.join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(cancelled.elapsed() < Duration::from_secs(2));
    assert_eq!(git(&f.root, &["write-tree"]), index);
    assert_eq!(fs::read(f.root.join("image.png")).unwrap(), f.pointer);
}
