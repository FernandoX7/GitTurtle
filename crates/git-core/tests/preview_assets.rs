use gitturtle_core::{GitRepository, HistoryCancellation, PreviewAssetScope, preview_asset_path};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        let fixture = Self { temp, root };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.name", "Asset Fixture"]);
        fixture.git(&["config", "user.email", "assets@example.invalid"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture.write("docs/guide.md", b"![image](../images/picture.png)\n");
        fixture.write("images/picture.png", b"committed image");
        fixture
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn write(&self, path: impl AsRef<Path>, bytes: &[u8]) {
        let target = self.root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
    fn commit(&self) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", "fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
}

#[test]
fn captured_commit_images_keep_revision_identity_after_refs_and_working_files_move() {
    let f = Fixture::new();
    let old = f.commit();
    let document = f.git(&["rev-parse", "HEAD:docs/guide.md"]);
    let image = f.git(&["rev-parse", "HEAD:images/picture.png"]);
    f.write("images/picture.png", b"new revision");
    f.commit();
    f.write("images/picture.png", b"later working image");
    let before_index = fs::read(f.root.join(".git/index")).unwrap();
    let assets = f
        .repo()
        .capture_preview_assets(
            &PreviewAssetScope::Revision(old),
            Path::new("docs/guide.md"),
            &["../images/picture.png".into()],
            Some(&document),
            1024,
            4096,
            &HistoryCancellation::default(),
        )
        .unwrap();
    let asset = assets[0].asset.as_ref().unwrap();
    assert_eq!(asset.bytes, b"committed image");
    assert_eq!(asset.blob_oid.as_deref(), Some(image.as_str()));
    assert_eq!(asset.path, Path::new("images/picture.png"));
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), before_index);
    assert_eq!(
        fs::read(f.root.join("images/picture.png")).unwrap(),
        b"later working image"
    );
}

#[test]
fn staged_and_working_images_are_separate_and_raw_filters_never_run() {
    let f = Fixture::new();
    f.commit();
    f.write("images/picture.png", b"staged image");
    f.git(&["add", "images/picture.png"]);
    f.write("images/picture.png", b"working image");
    f.write(".gitattributes", b"*.png filter=hostile diff=hostile\n");
    let marker = f.temp.path().join("helper-ran");
    let command = format!("touch '{}'; cat", marker.display());
    f.git(&["config", "filter.hostile.clean", &command]);
    f.git(&["config", "filter.hostile.smudge", &command]);
    f.git(&["config", "diff.hostile.textconv", &command]);
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let repo = f.repo();
    for (scope, expected, immutable) in [
        (PreviewAssetScope::Index, b"staged image".as_slice(), true),
        (
            PreviewAssetScope::Worktree,
            b"working image".as_slice(),
            false,
        ),
    ] {
        let assets = repo
            .capture_preview_assets(
                &scope,
                Path::new("docs/guide.md"),
                &["../images/picture.png".into()],
                None,
                1024,
                4096,
                &HistoryCancellation::default(),
            )
            .unwrap();
        let asset = assets[0].asset.as_ref().unwrap();
        assert_eq!(asset.bytes, expected);
        assert_eq!(asset.blob_oid.is_some(), immutable);
    }
    assert!(!marker.exists());
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
}

#[test]
fn normalization_preserves_encoded_names_and_refuses_resources_outside_repo() {
    assert_eq!(
        preview_asset_path(Path::new("docs/nested/guide.md"), "../../images/a%20b.png").unwrap(),
        Path::new("images/a b.png")
    );
    assert_eq!(
        preview_asset_path(Path::new("docs/guide.md"), "./a%23b.png").unwrap(),
        Path::new("docs/a#b.png")
    );
    for url in [
        "../../../escape.png",
        "https://example.invalid/a.png",
        "//example.invalid/a.png",
        "/tmp/a.png",
        "data:image/png;base64,AA",
        "file:///tmp/a.png",
        "%2fetc/passwd",
        "..%2f..%2fescape",
        "..%5cescape",
        "%00.png",
        "%zz.png",
        "../.git/config",
        "../.GiT/config",
        "pic.png?raw=1",
        "pic.svg#fragment",
    ] {
        assert!(
            preview_asset_path(Path::new("docs/guide.md"), url).is_err(),
            "{url}"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            preview_asset_path(Path::new("guide.md"), "%ff.png")
                .unwrap()
                .as_os_str()
                .as_bytes(),
            b"\xff.png"
        );
    }
}

#[test]
fn batch_keeps_per_image_errors_and_limits_aggregate_encoded_bytes() {
    let f = Fixture::new();
    f.write("images/small.png", b"ok");
    let oid = f.commit();
    let assets = f
        .repo()
        .capture_preview_assets(
            &PreviewAssetScope::Revision(oid),
            Path::new("docs/guide.md"),
            &[
                "../images/picture.png".into(),
                "../images/small.png".into(),
                "../../outside.png".into(),
                "../images/missing.png".into(),
            ],
            None,
            100,
            16,
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(assets[0].asset.as_ref().unwrap().bytes, b"committed image");
    assert!(assets[1].error.as_ref().unwrap().contains("budget"));
    assert!(assets[2].error.as_ref().unwrap().contains("escapes"));
    assert!(assets[3].error.as_ref().unwrap().contains("absent"));
    let cancelled = HistoryCancellation::default();
    cancelled.cancel();
    assert!(
        f.repo()
            .capture_preview_assets(
                &PreviewAssetScope::Worktree,
                Path::new("docs/guide.md"),
                &["../images/small.png".into()],
                None,
                100,
                100,
                &cancelled
            )
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn document_identity_and_full_commit_scope_are_revalidated() {
    let f = Fixture::new();
    f.commit();
    let old = f.git(&["rev-parse", "HEAD:docs/guide.md"]);
    f.write("docs/guide.md", b"replacement source");
    f.git(&["add", "docs/guide.md"]);
    let repo = f.repo();
    assert!(
        repo.capture_preview_assets(
            &PreviewAssetScope::Index,
            Path::new("docs/guide.md"),
            &["../images/picture.png".into()],
            Some(&old),
            100,
            100,
            &HistoryCancellation::default()
        )
        .unwrap_err()
        .to_string()
        .contains("identity changed")
    );
    for scope in ["HEAD".into(), f.git(&["rev-parse", "HEAD^{tree}"]), old] {
        assert!(
            repo.capture_preview_assets(
                &PreviewAssetScope::Revision(scope),
                Path::new("docs/guide.md"),
                &["../images/picture.png".into()],
                None,
                100,
                100,
                &HistoryCancellation::default()
            )
            .is_err()
        );
    }
}

#[cfg(unix)]
#[test]
fn stored_and_replaced_symlink_files_and_directories_are_never_followed() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.png"), b"outside secret").unwrap();
    symlink(outside.join("secret.png"), f.root.join("images/link.png")).unwrap();
    symlink(&outside, f.root.join("link-directory")).unwrap();
    f.write("nested/secret.png", b"tracked local");
    let oid = f.commit();
    let urls = vec![
        "../images/link.png".into(),
        "../link-directory/secret.png".into(),
    ];
    let repo = f.repo();
    for scope in [
        PreviewAssetScope::Revision(oid),
        PreviewAssetScope::Index,
        PreviewAssetScope::Worktree,
    ] {
        let assets = repo
            .capture_preview_assets(
                &scope,
                Path::new("docs/guide.md"),
                &urls,
                None,
                1024,
                4096,
                &HistoryCancellation::default(),
            )
            .unwrap();
        assert!(
            assets
                .iter()
                .all(|a| a.asset.is_none() && a.error.is_some())
        );
    }
    fs::remove_file(f.root.join("images/picture.png")).unwrap();
    symlink(
        outside.join("secret.png"),
        f.root.join("images/picture.png"),
    )
    .unwrap();
    fs::remove_dir_all(f.root.join("nested")).unwrap();
    symlink(&outside, f.root.join("nested")).unwrap();
    let assets = repo
        .capture_preview_assets(
            &PreviewAssetScope::Worktree,
            Path::new("docs/guide.md"),
            &[
                "../images/picture.png".into(),
                "../nested/secret.png".into(),
            ],
            None,
            1024,
            4096,
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert!(
        assets
            .iter()
            .all(|a| a.asset.is_none() && a.error.as_ref().unwrap().contains("symbolic-link"))
    );
    assert_eq!(
        fs::read(outside.join("secret.png")).unwrap(),
        b"outside secret"
    );
}

#[test]
fn literal_git_pathspec_names_do_not_expand_or_match_other_files() {
    let f = Fixture::new();
    f.write("images/[special]*.png", b"literal path");
    f.write("images/special-other.png", b"unrelated");
    let oid = f.commit();
    for scope in [
        PreviewAssetScope::Revision(oid),
        PreviewAssetScope::Index,
        PreviewAssetScope::Worktree,
    ] {
        let assets = f
            .repo()
            .capture_preview_assets(
                &scope,
                Path::new("docs/guide.md"),
                &["../images/%5bspecial%5d*.png".into()],
                None,
                100,
                100,
                &HistoryCancellation::default(),
            )
            .unwrap();
        assert_eq!(assets[0].asset.as_ref().unwrap().bytes, b"literal path");
    }
}
