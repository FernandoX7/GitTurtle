use gitturtle_core::{
    ConflictBlockChoice, ConflictResolution, GitRepository, IntegrationCommand,
    choose_conflict_block, text_conflict_blocks,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[test]
fn recovery_identity_survives_reopen_and_unrelated_staging_but_rejects_changed_sources() {
    let fixture = Fixture::new("diff3");
    let repo = GitRepository::open(&fixture.root).unwrap();
    let original = repo
        .conflict_preview(Path::new("file.txt"))
        .unwrap()
        .draft_identity();
    drop(repo);
    let reopened = GitRepository::open(&fixture.root).unwrap();
    assert_eq!(
        reopened
            .conflict_preview(Path::new("file.txt"))
            .unwrap()
            .draft_identity(),
        original
    );
    fs::write(fixture.root.join("unrelated.txt"), "preserve me\n").unwrap();
    fixture.git(&["add", "unrelated.txt"]);
    assert_eq!(
        reopened
            .conflict_preview(Path::new("file.txt"))
            .unwrap()
            .draft_identity(),
        original
    );
    fs::write(fixture.root.join("file.txt"), "external resolution\n").unwrap();
    assert_ne!(
        reopened
            .conflict_preview(Path::new("file.txt"))
            .unwrap()
            .draft_identity(),
        original
    );
    assert_eq!(fixture.git(&["show", ":unrelated.txt"]), "preserve me");
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim_end().into()
    }
    fn new(style: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("conflicts");
        fs::create_dir(&root).unwrap();
        let f = Self { _temp: temp, root };
        f.git(&["init", "-b", "main"]);
        f.git(&["config", "user.name", "Block Fixture"]);
        f.git(&["config", "user.email", "block@example.invalid"]);
        f.git(&["config", "commit.gpgsign", "false"]);
        f.git(&["config", "merge.conflictStyle", style]);
        f.write("base one", "base two");
        f.git(&["add", "."]);
        f.git(&["commit", "-m", "base"]);
        f.git(&["switch", "-c", "incoming"]);
        f.write("incoming one Ω", "incoming two");
        f.git(&["commit", "-am", "incoming"]);
        f.git(&["switch", "main"]);
        f.write("current one 🐢", "current two");
        f.git(&["commit", "-am", "current"]);
        let out = Command::new("git")
            .arg("-C")
            .arg(&f.root)
            .args(["merge", "incoming"])
            .output()
            .unwrap();
        assert!(!out.status.success());
        f
    }
    fn write(&self, first: &str, second: &str) {
        fs::write(
            self.root.join("file.txt"),
            format!(
                "{first}\n{}\n{second}\n",
                (0..12)
                    .map(|i| format!("unchanged {i}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )
        .unwrap();
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
}

#[test]
fn per_block_drafts_save_without_staging_then_revalidate_before_final_resolution() {
    for style in ["merge", "diff3", "zdiff3"] {
        let f = Fixture::new(style);
        let repo = f.repo();
        fs::write(f.root.join("unrelated"), "keep staged").unwrap();
        f.git(&["add", "unrelated"]);
        let preview = Arc::new(repo.conflict_preview(Path::new("file.txt")).unwrap());
        let source = String::from_utf8(preview.working.as_ref().unwrap().bytes.clone()).unwrap();
        let blocks = text_conflict_blocks(&source).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].base.is_some(), style != "merge");
        let first =
            choose_conflict_block(&source, &blocks[0], ConflictBlockChoice::Current).unwrap();
        assert!(
            repo.execute_integration(&IntegrationCommand::Resolve {
                expected: Arc::clone(&preview),
                resolution: ConflictResolution::Manual {
                    bytes: first.as_bytes().to_vec()
                }
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(f.root.join("file.txt")).unwrap(), source);
        assert!(
            repo.execute_integration(&IntegrationCommand::Resolve {
                expected: Arc::clone(&preview),
                resolution: ConflictResolution::MarkResolved
            })
            .is_err()
        );
        let index = f.git(&["ls-files", "--stage"]);
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::clone(&preview),
            resolution: ConflictResolution::Save {
                bytes: first.as_bytes().to_vec(),
            },
        })
        .unwrap();
        assert_eq!(f.git(&["ls-files", "--stage"]), index);
        assert_eq!(fs::read_to_string(f.root.join("file.txt")).unwrap(), first);
        let blocks = text_conflict_blocks(&first).unwrap();
        assert_eq!(blocks.len(), 1);
        let complete =
            choose_conflict_block(&first, &blocks[0], ConflictBlockChoice::Incoming).unwrap();
        assert!(
            repo.execute_integration(&IntegrationCommand::Resolve {
                expected: preview,
                resolution: ConflictResolution::Manual {
                    bytes: complete.as_bytes().to_vec()
                }
            })
            .unwrap_err()
            .to_string()
            .contains("changed")
        );
        let refreshed = Arc::new(repo.conflict_preview(Path::new("file.txt")).unwrap());
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: refreshed,
            resolution: ConflictResolution::Manual {
                bytes: complete.as_bytes().to_vec(),
            },
        })
        .unwrap();
        assert!(f.git(&["ls-files", "--unmerged"]).is_empty());
        assert_eq!(f.git(&["show", ":file.txt"]), complete.trim_end());
        assert_eq!(f.git(&["show", ":unrelated"]), "keep staged");
        assert!(complete.starts_with("current one 🐢\n") && complete.ends_with("incoming two\n"));
    }
}

#[test]
fn changed_sources_refuse_a_prepared_save_without_touching_newer_work() {
    let f = Fixture::new("merge");
    let repo = f.repo();
    let expected = Arc::new(repo.conflict_preview(Path::new("file.txt")).unwrap());
    fs::write(f.root.join("file.txt"), "external resolution\n").unwrap();
    let index = f.git(&["ls-files", "--stage"]);
    assert!(
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected,
            resolution: ConflictResolution::Save {
                bytes: b"old draft\n".to_vec()
            }
        })
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "external resolution\n"
    );
    assert_eq!(f.git(&["ls-files", "--stage"]), index);
}
