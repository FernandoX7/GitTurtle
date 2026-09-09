use gitturtle_core::{
    GitRepository, LfsDownloadTarget, OperationControl, WriteCommand, run_controlled,
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
        if !Command::new("git")
            .args(["lfs", "version"])
            .output()
            .ok()?
            .status
            .success()
        {
            eprintln!("Git LFS unavailable; local transfer fixture not run");
            return None;
        }
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
        Some(Self {
            _temp: temp,
            root,
            remote,
            pointer,
            payload,
            oid,
            blob,
        })
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
