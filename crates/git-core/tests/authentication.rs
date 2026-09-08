//! Loopback-only transport fixtures; these do not establish hosted-provider auth.
#![cfg(unix)]
use gitturtle_core::{GitRepository, OperationControl, WriteCommand, run_controlled};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    authorized: Arc<Mutex<usize>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start(root: PathBuf, accepted: bool) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let authorized = Arc::new(Mutex::new(0));
        let count = authorized.clone();
        let worker = thread::spawn(move || {
            let start = Instant::now();
            while !stopped.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(15) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(3));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 2048];
                while request.len() < 16384 && !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&bytes[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&request);
                // Basic base64("fixture:fixture-token"), disposable fixture only.
                let supplied = request.lines().any(|line| {
                    line.eq_ignore_ascii_case("Authorization: Basic Zml4dHVyZTpmaXh0dXJlLXRva2Vu")
                });
                if !supplied || !accepted {
                    let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=GitTurtleFixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                *count.lock().unwrap() += 1;
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap()
                    .trim_start_matches('/');
                let content = if path.contains("..") {
                    None
                } else {
                    fs::read(root.join(path)).ok()
                };
                let (status, body) =
                    content.map_or(("404 Not Found", Vec::new()), |bytes| ("200 OK", bytes));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Self {
            url,
            stop,
            authorized,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    source: PathBuf,
    bare: PathBuf,
    destination: PathBuf,
    helper_log: PathBuf,
    head: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let bare = temp.path().join("bare.git");
        let destination = temp.path().join("destination");
        GitRepository::init(&source, "main").unwrap();
        git(&source, &["config", "user.name", "Authentication fixture"]);
        git(
            &source,
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(&source, &["config", "commit.gpgsign", "false"]);
        git(&source, &["config", "core.hooksPath", ".git/hooks"]);
        fs::write(source.join("file.txt"), "remote contents\n").unwrap();
        git(&source, &["add", "file.txt"]);
        git(&source, &["commit", "-m", "fixture"]);
        let head = git(&source, &["rev-parse", "HEAD"]);
        git(
            &source,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        git(&bare, &["update-server-info"]);
        GitRepository::init(&destination, "main").unwrap();
        let helper = temp.path().join("credential-helper");
        let helper_log = temp.path().join("helper-log");
        // Helper receives Git's get/store/erase protocol. No real credential store.
        fs::write(&helper, format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\ncat >/dev/null\nif [ \"$1\" = get ]; then printf 'username=fixture\\npassword=fixture-token\\n\\n'; fi\n", helper_log.display())).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        git(&destination, &["config", "credential.helper", ""]);
        git(
            &destination,
            &[
                "config",
                "--add",
                "credential.helper",
                helper.to_str().unwrap(),
            ],
        );
        git(&destination, &["config", "http.proxy", ""]);
        Self {
            _temp: temp,
            source,
            bare,
            destination,
            helper_log,
            head,
        }
    }
}

#[test]
fn configured_helper_authenticates_loopback_fetch_and_pull_preserving_local_files() {
    let fixture = Fixture::new();
    let server = Server::start(fixture.bare.clone(), true);
    git(
        &fixture.destination,
        &["remote", "add", "origin", &server.url],
    );
    fs::write(fixture.destination.join("unrelated.txt"), "local draft").unwrap();
    let repo = GitRepository::open(&fixture.destination).unwrap();
    run_controlled(OperationControl::default(), || {
        repo.execute(&WriteCommand::Fetch {
            remote: "origin".into(),
        })
    })
    .unwrap();
    assert_eq!(
        git(
            &fixture.destination,
            &["rev-parse", "refs/remotes/origin/main"]
        ),
        fixture.head
    );
    run_controlled(OperationControl::default(), || {
        repo.execute(&WriteCommand::Pull {
            remote: "origin".into(),
            branch: "main".into(),
        })
    })
    .unwrap();
    assert_eq!(
        git(&fixture.destination, &["rev-parse", "HEAD"]),
        fixture.head
    );
    assert_eq!(
        fs::read_to_string(fixture.destination.join("unrelated.txt")).unwrap(),
        "local draft"
    );
    assert_eq!(
        fs::read_to_string(fixture.destination.join("file.txt")).unwrap(),
        "remote contents\n"
    );
    let calls = fs::read_to_string(&fixture.helper_log).unwrap();
    assert!(calls.lines().any(|call| call == "get"));
    assert!(calls.lines().any(|call| call == "store"));
    assert!(*server.authorized.lock().unwrap() > 0);
    assert!(!format!("{:?}", repo.remotes().unwrap()).contains("fixture-token"));
}

#[test]
fn expired_loopback_credentials_are_rejected_by_helper_and_error_is_actionable() {
    let fixture = Fixture::new();
    let server = Server::start(fixture.bare.clone(), false);
    git(
        &fixture.destination,
        &["remote", "add", "origin", &server.url],
    );
    let repo = GitRepository::open(&fixture.destination).unwrap();
    let error = run_controlled(OperationControl::default(), || {
        repo.execute(&WriteCommand::Fetch {
            remote: "origin".into(),
        })
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("expired") && error.contains("explicitly retry"),
        "{error}"
    );
    assert!(!error.contains("fixture-token"), "{error}");
    let calls = fs::read_to_string(&fixture.helper_log).unwrap();
    assert_eq!(calls.lines().filter(|call| *call == "get").count(), 1);
    assert_eq!(calls.lines().filter(|call| *call == "erase").count(), 1);
    assert!(repo.history(1).unwrap().is_empty());
}

#[test]
fn clone_refuses_embedded_secrets_before_creating_destination() {
    let fixture = Fixture::new();
    let destination = fixture.source.join("must-not-exist");
    let error =
        GitRepository::clone_repository("https://fixture:secret@127.0.0.1/repo", &destination)
            .unwrap_err()
            .to_string();
    assert!(error.contains("without embedded credentials"));
    assert!(!error.contains("fixture:secret"));
    assert!(!destination.exists());
}
