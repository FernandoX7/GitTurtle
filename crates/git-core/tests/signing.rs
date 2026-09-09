//! Real signing fixtures use generated keys and isolated agents only. They never
//! access the user's SSH socket, keyring, credentials, or global Git settings.
#![cfg(unix)]
use gitturtle_core::{GitRepository, TagCommand, WriteCommand};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    repo: GitRepository,
    agent: Option<Child>,
    server: Option<Child>,
    gpg_home: Option<std::path::PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        GitRepository::init(&root, "main").unwrap();
        let result = Self {
            temp,
            repo: GitRepository::open(&root).unwrap(),
            agent: None,
            server: None,
            gpg_home: None,
        };
        for (key, value) in [
            ("user.name", "Signing Fixture"),
            ("user.email", "signer@example.invalid"),
            ("commit.gpgsign", "true"),
            ("tag.gpgsign", "true"),
            ("core.attributesFile", "/dev/null"),
        ] {
            result.git(&["config", key, value]);
        }
        fs::create_dir(result.temp.path().join("hooks")).unwrap();
        result.git(&[
            "config",
            "core.hooksPath",
            result.temp.path().join("hooks").to_str().unwrap(),
        ]);
        fs::write(result.repo.path().join("tracked.txt"), "reviewed content\n").unwrap();
        result.repo.execute(&WriteCommand::StageAll).unwrap();
        result
    }
    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "SSH_AUTH_SOCK",
            "SSH_AGENT_PID",
            "GPG_TTY",
        ] {
            cmd.env_remove(name);
        }
        cmd
    }
    fn git(&self, args: &[&str]) -> String {
        let output = self
            .command("git")
            .arg("-C")
            .arg(self.repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    fn script(&self, name: &str, text: &str) -> std::path::PathBuf {
        let path = self.temp.path().join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn isolated_ssh_agent(&mut self) -> (std::path::PathBuf, std::path::PathBuf) {
        let key = self.temp.path().join("generated-signing-key");
        let output = self
            .command("ssh-keygen")
            .args([
                "-q",
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "signer@example.invalid",
                "-f",
            ])
            .arg(&key)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let socket = self.temp.path().join("agent.sock");
        self.agent = Some(
            self.command("ssh-agent")
                .arg("-D")
                .arg("-a")
                .arg(&socket)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            assert!(
                Instant::now() < deadline,
                "isolated SSH agent did not start"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let output = self
            .command("ssh-add")
            .env("SSH_AUTH_SOCK", &socket)
            .arg(&key)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let public = key.with_extension("pub");
        // With the private file gone, operations must use only this agent.
        fs::remove_file(&key).unwrap();
        (public, socket)
    }
    fn signed_commit_and_tag(&self) {
        let hook = self.temp.path().join("hooks/commit-msg");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf 'called\\n' >> .git/signing-hook-calls\n",
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        self.repo
            .execute(&WriteCommand::Commit {
                message: "Signed by fixture agent".into(),
            })
            .unwrap();
        self.git(&["verify-commit", "HEAD"]);
        assert_eq!(
            fs::read_to_string(self.repo.path().join(".git/signing-hook-calls")).unwrap(),
            "called\n"
        );
        let plan = self
            .repo
            .create_tag_plan(
                "signed-fixture",
                "HEAD",
                Some("Reviewed release annotation\n".into()),
            )
            .unwrap();
        assert!(plan.signing);
        self.repo
            .execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .unwrap();
        self.git(&["verify-tag", "signed-fixture"]);
        assert!(
            self.git(&["cat-file", "tag", "signed-fixture"])
                .contains("Reviewed release annotation\n")
        );
    }
    fn stop_agent(&mut self) {
        if let Some(mut agent) = self.agent.take() {
            let _ = agent.kill();
            let _ = agent.wait();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop_agent();
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        if let Some(keyring) = &self.gpg_home {
            let _ = self
                .command("gpgconf")
                .arg("--homedir")
                .arg(keyring)
                .args(["--kill", "gpg-agent"])
                .output();
        }
    }
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[test]
fn configured_ssh_agent_signs_real_commits_and_tags_without_unsigned_fallback() {
    let mut f = Fixture::new();
    let (public, socket) = f.isolated_ssh_agent();
    let signers = f.temp.path().join("allowed-signers");
    fs::write(
        &signers,
        format!(
            "signer@example.invalid {}",
            fs::read_to_string(&public).unwrap()
        ),
    )
    .unwrap();
    let signer = f.script(
        "ssh-signing-program",
        &format!(
            "#!/bin/sh\nexport SSH_AUTH_SOCK={}\nexec ssh-keygen \"$@\"\n",
            quote(&socket)
        ),
    );
    for (name, value) in [
        ("gpg.format", "ssh"),
        ("user.signingkey", public.to_str().unwrap()),
        ("gpg.ssh.program", signer.to_str().unwrap()),
        ("gpg.ssh.allowedSignersFile", signers.to_str().unwrap()),
    ] {
        f.git(&["config", name, value]);
    }
    f.signed_commit_and_tag();
    let before = f.git(&["rev-parse", "HEAD"]);
    f.stop_agent();
    fs::write(f.repo.path().join("tracked.txt"), "next reviewed content\n").unwrap();
    f.repo.execute(&WriteCommand::StageAll).unwrap();
    let index = f.git(&["ls-files", "--stage"]);
    assert!(
        f.repo
            .execute(&WriteCommand::Commit {
                message: "Must not become unsigned".into()
            })
            .is_err()
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(f.git(&["ls-files", "--stage"]), index);
    assert_eq!(
        fs::read_to_string(f.repo.path().join("tracked.txt")).unwrap(),
        "next reviewed content\n"
    );
    let plan = f
        .repo
        .create_tag_plan("must-not-exist", "HEAD", Some("Signing unavailable".into()))
        .unwrap();
    assert!(
        f.repo
            .execute(&WriteCommand::Tag(Arc::new(TagCommand::Create(plan))))
            .is_err()
    );
    assert_eq!(f.git(&["tag", "--list", "must-not-exist"]), "");
}

#[test]
#[ignore = "Requires GnuPG; run explicitly to verify configured OpenPGP signing with an isolated generated keyring"]
fn configured_openpgp_signs_real_commits_and_tags_in_an_isolated_keyring() {
    let mut f = Fixture::new();
    let keyring = f.temp.path().join("gnupg");
    f.gpg_home = Some(keyring.clone());
    fs::create_dir(&keyring).unwrap();
    fs::set_permissions(&keyring, fs::Permissions::from_mode(0o700)).unwrap();
    let output = f
        .command("gpg")
        .arg("--homedir")
        .arg(&keyring)
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "",
            "--quick-generate-key",
            "Signing Fixture <signer@example.invalid>",
            "ed25519",
            "sign",
            "1d",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = f
        .command("gpg")
        .arg("--homedir")
        .arg(&keyring)
        .args(["--with-colons", "--list-secret-keys"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = String::from_utf8(output.stdout).unwrap();
    let fingerprint = data
        .lines()
        .find(|line| line.starts_with("fpr:"))
        .unwrap()
        .split(':')
        .nth(9)
        .unwrap();
    let signer = f.script(
        "gpg-signing-program",
        &format!(
            "#!/bin/sh\nexec gpg --homedir {} --batch --pinentry-mode loopback \"$@\"\n",
            quote(&keyring)
        ),
    );
    for (key, value) in [
        ("gpg.format", "openpgp"),
        ("user.signingkey", fingerprint),
        ("gpg.program", signer.to_str().unwrap()),
    ] {
        f.git(&["config", key, value]);
    }
    f.signed_commit_and_tag();
}

#[test]
#[ignore = "Requires a runnable local sshd; binds only loopback and uses generated fixture keys with strict host verification"]
fn configured_ssh_agent_fetches_over_loopback_and_rejects_unknown_hosts() {
    assert!(
        std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none(),
        "Unset inherited SSH overrides for this isolated fixture"
    );
    let mut f = Fixture::new();
    let (public, socket) = f.isolated_ssh_agent();
    f.git(&["config", "commit.gpgsign", "false"]);
    f.repo
        .execute(&WriteCommand::Commit {
            message: "Loopback transport fixture".into(),
        })
        .unwrap();
    let source_head = f.git(&["rev-parse", "HEAD"]);
    let bare = f.temp.path().join("remote.git");
    f.git(&[
        "clone",
        "--bare",
        f.repo.path().to_str().unwrap(),
        bare.to_str().unwrap(),
    ]);
    let host = f.temp.path().join("host-key");
    let output = f
        .command("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&host)
        .output()
        .unwrap();
    assert!(output.status.success());
    let authorized = f.temp.path().join("authorized_keys");
    fs::copy(&public, &authorized).unwrap();
    fs::set_permissions(&authorized, fs::Permissions::from_mode(0o600)).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let output = f.command("id").arg("-un").output().unwrap();
    assert!(output.status.success());
    let user = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let config = f.temp.path().join("sshd_config");
    fs::write(&config, format!("Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPidFile {}\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nUsePAM no\nStrictModes yes\nAllowUsers {user}\n", host.display(), authorized.display(), f.temp.path().join("sshd.pid").display())).unwrap();
    let output = f
        .command("/usr/sbin/sshd")
        .arg("-t")
        .arg("-f")
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    f.server = Some(
        f.command("/usr/sbin/sshd")
            .arg("-D")
            .arg("-e")
            .arg("-f")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "loopback sshd did not start");
        thread::sleep(Duration::from_millis(10));
    }
    let host_public = fs::read_to_string(host.with_extension("pub")).unwrap();
    let fields = host_public.split_whitespace().collect::<Vec<_>>();
    let known = f.temp.path().join("known_hosts");
    fs::write(
        &known,
        format!("[127.0.0.1]:{port} {} {}\n", fields[0], fields[1]),
    )
    .unwrap();
    let ssh = f.script("transport-program", &format!("#!/bin/sh\nexport SSH_AUTH_SOCK={}\nexec ssh -F /dev/null -o BatchMode=yes -o StrictHostKeyChecking=yes -o GlobalKnownHostsFile=/dev/null -o UserKnownHostsFile={} -o IdentitiesOnly=yes -i {} \"$@\"\n", quote(&socket), quote(&known), quote(&public)));
    // Fetch into the original local fixture via its separately captured bare
    // source. Both repositories and every object stay under the temporary root.
    f.git(&["config", "core.sshCommand", ssh.to_str().unwrap()]);
    f.git(&[
        "remote",
        "add",
        "fixture",
        &format!("ssh://{user}@127.0.0.1:{port}{}", bare.display()),
    ]);
    fs::write(f.repo.path().join("unrelated.txt"), "keep this draft\n").unwrap();
    f.repo
        .execute(&WriteCommand::Fetch {
            remote: "fixture".into(),
        })
        .unwrap();
    assert_eq!(
        f.git(&["rev-parse", "refs/remotes/fixture/main"]),
        source_head
    );
    assert_eq!(
        fs::read_to_string(f.repo.path().join("unrelated.txt")).unwrap(),
        "keep this draft\n"
    );
    fs::write(&known, "").unwrap();
    let error = f
        .repo
        .execute(&WriteCommand::Fetch {
            remote: "fixture".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(
        error.to_lowercase().contains("host key") || error.to_lowercase().contains("verification"),
        "{error}"
    );
    assert_eq!(
        f.git(&["rev-parse", "refs/remotes/fixture/main"]),
        source_head
    );
    assert_eq!(
        fs::read_to_string(f.repo.path().join("unrelated.txt")).unwrap(),
        "keep this draft\n"
    );
}
