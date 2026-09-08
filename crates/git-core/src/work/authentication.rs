//! Ephemeral askpass for explicitly initiated operations. Credentials only cross
//! a private local socket and live in memory until this operation ends.
use super::*;
use std::{
    cell::RefCell,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const PROMPT_LIMIT: usize = 8192;
const RESPONSE_LIMIT: usize = 16 * 1024;
const SOCKET_ENV: &str = "GITTURTLE_ASKPASS_SOCKET";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticationPrompt {
    pub id: u64,
    pub message: String,
    pub secret: bool,
    pub confirmation: bool,
}

#[derive(Default)]
struct State {
    prompt: Option<AuthenticationPrompt>,
    delivered: bool,
    answer: Option<String>,
    secrets: Vec<String>,
    progress: Option<String>,
}
#[derive(Default)]
struct Shared {
    cancelled: AtomicBool,
    sequence: AtomicU64,
    state: Mutex<State>,
    changed: Condvar,
}
/// Explicit cancellation is separate from dropping a reply: accepted writes
/// still run exactly once unless the user requests cancellation.
#[derive(Clone, Default)]
pub struct OperationControl(Arc<Shared>);

impl OperationControl {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.changed.notify_all();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn take_prompt(&self) -> Option<AuthenticationPrompt> {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.delivered {
            return None;
        }
        let prompt = state.prompt.clone()?;
        state.delivered = true;
        Some(prompt)
    }
    pub fn answer(&self, id: u64, answer: Option<String>) -> bool {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.prompt.as_ref().is_none_or(|p| p.id != id)
            || state.answer.is_some()
            || self.is_cancelled()
        {
            return false;
        }
        if let Some(answer) = answer {
            if answer.len() > RESPONSE_LIMIT || answer.contains(['\n', '\r', '\0']) {
                return false;
            }
            if state.prompt.as_ref().is_some_and(|p| p.secret) && !answer.is_empty() {
                state.secrets.push(answer.clone());
            }
            state.answer = Some(answer);
        } else {
            self.cancel();
        }
        self.0.changed.notify_all();
        true
    }
    pub fn progress(&self) -> Option<String> {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .progress
            .clone()
    }
    pub(super) fn update_progress(&self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        // Progress is deliberately restricted to Git's numeric transfer phases.
        // Arbitrary helper/hook output belongs only in redacted error details.
        if let Some(line) = text.split(['\n', '\r']).rev().find(|line| {
            [
                "Receiving objects:",
                "Counting objects:",
                "Enumerating objects:",
                "Compressing objects:",
                "Writing objects:",
                "Resolving deltas:",
            ]
            .iter()
            .any(|prefix| line.trim_start().starts_with(prefix))
        }) {
            // Redact exact answers before acquiring the progress lock: redact()
            // reads the same state, including previously entered secrets.
            let progress = self.redact(line.trim()).chars().take(160).collect();
            self.0
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .progress = Some(progress);
        }
    }
    fn request(&self, message: String, stop: &AtomicBool) -> Option<String> {
        let id = self.0.sequence.fetch_add(1, Ordering::Relaxed);
        if id >= 16 {
            self.cancel();
            return None;
        }
        let lower = message.to_ascii_lowercase();
        let confirmation = lower.contains("yes/no")
            || lower.contains("(yes/no/")
            || lower.contains("fingerprint)");
        let prompt = AuthenticationPrompt {
            id,
            secret: !confirmation && !lower.starts_with("username"),
            confirmation,
            message: self.redact(&message),
        };
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.prompt = Some(prompt);
        state.delivered = false;
        state.answer = None;
        let deadline = Instant::now();
        let answer = loop {
            if self.is_cancelled()
                || stop.load(Ordering::Acquire)
                || deadline.elapsed() > NETWORK_TIMEOUT
            {
                break None;
            }
            if let Some(answer) = state.answer.take() {
                break Some(answer);
            }
            state = self
                .0
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        state.prompt = None;
        answer
    }
    fn redact(&self, input: &str) -> String {
        let state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut output = input.to_owned();
        for secret in &state.secrets {
            output = output.replace(secret, "[redacted]");
        }
        redact_diagnostic(&output)
    }
}

thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }
#[derive(Clone)]
struct Active {
    control: OperationControl,
    socket: Option<PathBuf>,
}
pub(super) fn current_control() -> Option<OperationControl> {
    ACTIVE.with_borrow(|a| a.as_ref().map(|a| a.control.clone()))
}
pub(super) fn is_controlled() -> bool {
    current_control().is_some()
}
pub(super) fn redact_current(value: &str) -> String {
    current_control().map_or_else(|| redact_diagnostic(value), |c| c.redact(value))
}

/// Run on the serialized worker. This never retries or changes Git configuration.
pub fn run_controlled<T>(
    control: OperationControl,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    #[cfg(unix)]
    let server = AskpassServer::new(control.clone())?;
    #[cfg(unix)]
    let socket = Some(server.socket.clone());
    #[cfg(not(unix))]
    let socket = None;
    let previous = ACTIVE.replace(Some(Active {
        control: control.clone(),
        socket,
    }));
    struct Restore(Option<Active>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.set(self.0.take());
        }
    }
    let _restore = Restore(previous);
    if control.is_cancelled() {
        bail!("Git operation cancelled before starting. It was not retried.");
    }
    operation().map_err(|error| anyhow::anyhow!("{}", control.redact(&format!("{error:#}"))))
}

pub(super) fn configure_environment(command: &mut Command) {
    // Inherited debug tracing can write credential exchanges to arbitrary files.
    for key in [
        "GIT_TRACE",
        "GIT_TRACE_PACKET",
        "GIT_TRACE_CURL",
        "GIT_CURL_VERBOSE",
        "GIT_TRACE_SETUP",
        "GIT_TRACE_PERFORMANCE",
        "GIT_TRACE2",
        "GIT_TRACE2_EVENT",
        "GIT_TRACE2_PERF",
        "GCM_TRACE",
        "GCM_TRACE_SECRETS",
    ] {
        command.env_remove(key);
    }
    if !is_controlled() {
        command
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_ASKPASS", "/usr/bin/false")
            .env("SSH_ASKPASS", "/usr/bin/false")
            .env("SSH_ASKPASS_REQUIRE", "force");
    }
}
pub(super) fn configure_askpass(command: &mut Command, configured_git_askpass: bool) -> Result<()> {
    let active = ACTIVE.with_borrow(Clone::clone);
    let Some(Active {
        socket: Some(socket),
        ..
    }) = active
    else {
        return Ok(());
    };
    let executable =
        std::env::current_exe().context("Cannot locate GitTurtle authentication helper")?;
    command.env(SOCKET_ENV, socket);
    if std::env::var_os("GIT_ASKPASS").is_none() && !configured_git_askpass {
        command.env("GIT_ASKPASS", &executable);
    }
    if std::env::var_os("SSH_ASKPASS").is_none() {
        command.env("SSH_ASKPASS", &executable);
    }
    // Askpass supplies the UI in a GUI process with no controlling terminal.
    // Existing programs and host-verification policy remain in charge.
    if std::env::var_os("SSH_ASKPASS_REQUIRE").is_none() {
        command.env("SSH_ASKPASS_REQUIRE", "force");
    }
    Ok(())
}

/// Call before starting the GUI. None denotes a normal application launch.
/// Askpass mode never loads preferences, renders a window or writes diagnostics.
pub fn run_askpass_if_requested() -> Option<i32> {
    let socket = std::env::var_os(SOCKET_ENV)?;
    #[cfg(unix)]
    {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        Some(match askpass_exchange(Path::new(&socket), &prompt) {
            Ok(answer) => {
                if std::io::stdout().write_all(&answer).is_ok()
                    && std::io::stdout().write_all(b"\n").is_ok()
                {
                    0
                } else {
                    1
                }
            }
            Err(_) => 1,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = socket;
        Some(1)
    }
}

#[cfg(unix)]
fn askpass_exchange(socket: &Path, prompt: &str) -> Result<Vec<u8>> {
    use std::os::unix::net::UnixStream;
    ensure!(
        prompt.len() <= PROMPT_LIMIT,
        "Authentication prompt is too long"
    );
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(NETWORK_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&(prompt.len() as u32).to_be_bytes())?;
    stream.write_all(prompt.as_bytes())?;
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    ensure!(size <= RESPONSE_LIMIT, "Authentication cancelled");
    let mut response = vec![0; size];
    stream.read_exact(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
struct AskpassServer {
    directory: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
#[cfg(unix)]
impl AskpassServer {
    fn new(control: OperationControl) -> Result<Self> {
        use std::os::unix::{fs::DirBuilderExt, net::UnixListener};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Short path also avoids macOS's small sockaddr_un path limit.
        let directory = PathBuf::from("/tmp").join(format!(
            "gitturtle-auth-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("askpass");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = std::fs::remove_dir(&directory);
                return Err(error.into());
            }
        };
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) && !control.is_cancelled() {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let handle = || -> Result<()> {
                            stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                            let mut size = [0; 4];
                            stream.read_exact(&mut size)?;
                            let size = u32::from_be_bytes(size) as usize;
                            ensure!(size <= PROMPT_LIMIT, "Prompt too long");
                            let mut prompt = vec![0; size];
                            stream.read_exact(&mut prompt)?;
                            let response = control.request(String::from_utf8(prompt)?, &stopped);
                            if let Some(response) = response {
                                stream.write_all(&(response.len() as u32).to_be_bytes())?;
                                stream.write_all(response.as_bytes())?;
                            } else {
                                stream.write_all(&u32::MAX.to_be_bytes())?;
                            }
                            Ok(())
                        };
                        let _ = {
                            let mut handle = handle;
                            handle()
                        };
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            directory,
            socket,
            stop,
            worker: Some(worker),
        })
    }
}
#[cfg(unix)]
impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

/// Remove userinfo and query/fragment values from URL-bearing Git diagnostics.
/// Also redact common credential assignment/header forms. Paths are untouched.
pub fn redact_diagnostic(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for part in input.split_inclusive(char::is_whitespace) {
        let Some(start) = part.find("://") else {
            result.push_str(part);
            continue;
        };
        let authority_start = start + 3;
        let authority_end = part[authority_start..]
            .find(['/', '?', '#', '\'', '"', '\n', '\r', ' '])
            .map_or(part.len(), |i| authority_start + i);
        let authority = &part[authority_start..authority_end];
        result.push_str(&part[..authority_start]);
        if let Some((_, host)) = authority.rsplit_once('@') {
            result.push_str("[redacted]@");
            result.push_str(host);
        } else {
            result.push_str(authority);
        }
        let remainder = &part[authority_end..];
        if let Some(query) = remainder.find(['?', '#']) {
            result.push_str(&remainder[..query]);
            result.push_str("?[redacted]");
            let suffix = remainder.trim_end_matches(char::is_whitespace);
            result.push_str(&remainder[suffix.len()..]);
        } else {
            result.push_str(remainder);
        }
    }
    result
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            for marker in [
                "authorization:",
                "proxy-authorization:",
                "password=",
                "passwd=",
                "access_token=",
                "refresh_token=",
                "token=",
                "oauth_token=",
            ] {
                if let Some(index) = lower.find(marker) {
                    return format!("{}[redacted]", &line[..index + marker.len()]);
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn validate_clone_address(source: &str) -> Result<()> {
    if let Some((scheme, rest)) = source.split_once("://")
        && matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
    {
        let authority = rest.split('/').next().unwrap_or(rest);
        ensure!(
            !authority.contains('@') && !rest.contains(['?', '#']),
            "Use a repository URL without embedded credentials, query parameters, or fragments. Git will request credentials through your configured helper or the authentication prompt."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_redact_urls_headers_and_credentials_but_keep_useful_paths() {
        let original = "fatal: https://user:secret@example.invalid/repo.git?token=sekret failed\nAuthorization: Bearer supersecret\npassword=fixture-password\nlocal/file.txt";
        let result = redact_diagnostic(original);
        for secret in [
            "user:",
            "secret@",
            "sekret",
            "supersecret",
            "fixture-password",
        ] {
            assert!(!result.contains(secret), "{result}");
        }
        assert!(result.contains("example.invalid/repo.git"));
        assert!(result.contains("local/file.txt"));
        assert_eq!(
            redact_diagnostic("İ Straße Authorization: secret"),
            "İ Straße Authorization:[redacted]"
        );
        assert_eq!(
            redact_diagnostic("İ password=secret"),
            "İ password=[redacted]"
        );
        assert!(validate_clone_address("https://user:password@example.invalid/repo").is_err());
        assert!(validate_clone_address("https://example.invalid/repo?token=secret").is_err());
        assert!(validate_clone_address("git@example.invalid:repo").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn native_askpass_exchange_keeps_secret_ephemeral_and_rejects_stale_answers() {
        let control = OperationControl::default();
        let server = AskpassServer::new(control.clone()).unwrap();
        let socket = server.socket.clone();
        let directory = server.directory.clone();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let helper = thread::spawn(move || {
            askpass_exchange(
                &socket,
                "Password for 'https://fixture@example.invalid/repo': ",
            )
            .unwrap()
        });
        let started = Instant::now();
        let prompt = loop {
            if let Some(prompt) = control.take_prompt() {
                break prompt;
            }
            assert!(started.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(5));
        };
        assert!(prompt.secret);
        assert!(!prompt.message.contains("fixture@"));
        assert!(!control.answer(prompt.id + 1, Some("wrong prompt".into())));
        assert!(control.answer(prompt.id, Some("ephemeral-fixture-token".into())));
        assert_eq!(helper.join().unwrap(), b"ephemeral-fixture-token");
        control.update_progress(b"Receiving objects: 50% ephemeral-fixture-token\r");
        let progress = control.progress().unwrap();
        assert!(!progress.contains("ephemeral-fixture-token"), "{progress}");
        assert!(progress.contains("50%"), "{progress}");
        assert!(
            !control
                .redact("helper echoed ephemeral-fixture-token")
                .contains("ephemeral-fixture-token")
        );
        drop(server);
        assert!(!directory.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_askpass_unblocks_helper_and_removes_private_socket() {
        let control = OperationControl::default();
        let server = AskpassServer::new(control.clone()).unwrap();
        let socket = server.socket.clone();
        let helper = thread::spawn(move || askpass_exchange(&socket, "Enter passphrase for key:"));
        let started = Instant::now();
        while control.take_prompt().is_none() {
            assert!(started.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(5));
        }
        control.cancel();
        assert!(helper.join().unwrap().is_err());
        drop(server);
    }

    #[cfg(unix)]
    #[test]
    fn explicit_cancel_terminates_pipe_holding_descendant_without_replaying_write() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("write-marker");
        let control = OperationControl::default();
        let worker_control = control.clone();
        let path = marker.clone();
        let worker = thread::spawn(move || {
            run_controlled(worker_control, || {
                let mut command = Command::new("sh");
                command
                    .arg("-c")
                    .arg("printf 'applied' > \"$1\"; sleep 30 & wait")
                    .arg("fixture")
                    .arg(path);
                bounded_write_output(command, None, Duration::from_secs(10))
            })
        });
        let start = Instant::now();
        while !marker.exists() {
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(5));
        }
        control.cancel();
        let error = worker.join().unwrap().unwrap_err().to_string();
        assert!(
            error.contains("may have applied partially or remotely"),
            "{error}"
        );
        assert!(error.contains("not retried"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "applied");
    }

    #[cfg(unix)]
    #[test]
    fn controlled_git_preserves_configured_askpass_and_helper_interaction() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("askpass");
        std::fs::write(&helper, "#!/bin/sh\ncase \"$1\" in Username*) printf 'fixture-user' ;; *) printf 'fixture-password' ;; esac\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        run_controlled(OperationControl::default(), || {
            let mut command = normal_command(directory.path());
            command
                .args(["-c", "credential.helper=", "-c"])
                .arg(format!("core.askPass={}", helper.display()))
                .args(["credential", "fill"]);
            configure_askpass(&mut command, true)?;
            let output = bounded_write_output(
                command,
                Some(b"protocol=https\nhost=fixture.invalid\n\n".to_vec()),
                Duration::from_secs(3),
            )?;
            ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let values = String::from_utf8(output.stdout)?;
            ensure!(
                values.contains("username=fixture-user")
                    && values.contains("password=fixture-password"),
                "Configured askpass was not used"
            );
            Ok(())
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_joins_io_threads_when_detached_helper_keeps_pipes_open() {
        use rustix::process::{Pid, Signal, kill_process};
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Ok(pid) = std::fs::read_to_string(&self.0)
                    && let Ok(pid) = pid.parse::<i32>()
                    && let Some(pid) = Pid::from_raw(pid)
                {
                    let _ = kill_process(pid, Signal::KILL);
                }
            }
        }
        // Check both a reader held after Git exits and a blocked large stdin
        // write retained by a helper that has closed its own output streams.
        for input_held in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let marker = directory.path().join("detached-helper-pid");
            let _cleanup = Cleanup(marker.clone());
            let control = OperationControl::default();
            let worker_control = control.clone();
            let path = marker.clone();
            let worker = thread::spawn(move || {
                run_controlled(worker_control, || {
                    let mut command = Command::new("python3");
                    command
                        .args([
                            "-c",
                            "import os,sys,time
if os.fork() == 0:
 os.setsid()
 if sys.argv[2] == 'stdin':
  os.close(1)
  os.close(2)
 with open(sys.argv[1], 'w') as file: file.write(str(os.getpid()))
 time.sleep(30)
 os._exit(0)
os._exit(0)
",
                        ])
                        .arg(path)
                        .arg(if input_held { "stdin" } else { "output" });
                    bounded_write_output(
                        command,
                        input_held.then(|| vec![b'x'; 1024 * 1024]),
                        Duration::from_secs(10),
                    )
                })
            });
            let started = Instant::now();
            while !std::fs::read_to_string(&marker).is_ok_and(|pid| !pid.is_empty()) {
                assert!(started.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(5));
            }
            let cancelled = Instant::now();
            control.cancel();
            let error = worker.join().unwrap().unwrap_err().to_string();
            assert!(
                error.contains("may have applied partially or remotely"),
                "{error}"
            );
            assert!(error.contains("not retried"), "{error}");
            assert!(
                cancelled.elapsed() < Duration::from_secs(2),
                "Cancellation waited on a detached helper: {:?}",
                cancelled.elapsed()
            );
        }
    }
}
