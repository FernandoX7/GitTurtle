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
                            // macOS accept(2) inherits the listener's O_NONBLOCK, which
                            // would drop a prompt the helper has not finished writing.
                            stream.set_nonblocking(false)?;
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
/// Also redact common credential assignment/header forms, wherever they appear:
/// a path holding one is cut like any other text. A marker with only
/// whitespace after it on its line names no secret and is left alone: it is
/// how a prompt such as OpenSSH's `user@host's password: ` ends. The line
/// ends at its `\n`, so a value after a carriage return is still cut.
pub fn redact_diagnostic(input: &str) -> String {
    hide_url_credentials(input)
        .lines()
        .map(|line| match first_credential_marker_end(line) {
            Some(end) if !line[end..].trim().is_empty() => {
                format!("{}[redacted]", &line[..end])
            }
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the first credential marker in `text` ends, wherever it falls in the
/// list: taking the list's first left the secret after an earlier marker on
/// the line, as in "token=secret Authorization:". A marker inside another
/// ("token=" in "access_token=") ends where the other does, so both keep the
/// same text. Lowercasing only ASCII keeps every byte offset of `text`.
fn first_credential_marker_end(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    [
        "authorization:",
        "proxy-authorization:",
        "password=",
        "password:",
        "passwd=",
        "passwd:",
        "access_token=",
        "access_token:",
        "refresh_token=",
        "refresh_token:",
        "token=",
        "token:",
        "oauth_token=",
        "oauth_token:",
    ]
    .into_iter()
    .filter_map(|marker| lower.find(marker).map(|index| index + marker.len()))
    .min()
}

/// `input` with the userinfo and the query of every URL hidden. A URL starts
/// at each `://`, several to a whitespace-separated word included; a query
/// runs to the word's end and so hides any later URL in it.
fn hide_url_credentials(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut copied = 0;
    // The first whitespace after the last authority, kept so that a word of
    // many URLs is scanned once.
    let mut word_end = 0;
    let mut previous_start = None;
    while let Some(found) = input[copied..].find("://") {
        let start = copied + found + 3;
        result.push_str(&input[copied..start]);
        let later_in_word = previous_start
            .is_some_and(|previous: usize| !input[previous..start].contains(char::is_whitespace));
        previous_start = Some(start);
        let end = start + url_authority_len(&input[start..]);
        let authority = &input[start..end];
        if let Some((userinfo, host)) = authority.rsplit_once('@') {
            // A word's first URL keeps its host whatever its userinfo spells
            // (`https://x-access-token:secret@host`). A later one's userinfo,
            // or one that whitespace runs through and so reaches past the
            // word, would otherwise take a marker away from the line's cut,
            // and what follows a marker is never shown, so it cuts the line
            // here.
            if (later_in_word || userinfo.contains(char::is_whitespace))
                && first_credential_marker_end(userinfo).is_some()
            {
                result.push_str("[redacted]");
                copied = input[end..].find('\n').map_or(input.len(), |i| end + i);
                continue;
            }
            result.push_str("[redacted]@");
            result.push_str(host);
        } else {
            result.push_str(authority);
        }
        if end > word_end {
            word_end = input[end..]
                .find(char::is_whitespace)
                .map_or(input.len(), |i| end + i);
        }
        let rest = &input[end..word_end];
        let next_url = rest.find("://").unwrap_or(rest.len());
        if let Some(query) = rest[..next_url].find(['?', '#']) {
            result.push_str(&rest[..query]);
            result.push_str("?[redacted]");
            copied = word_end;
        } else {
            result.push_str(&rest[..next_url]);
            copied = end + next_url;
        }
    }
    result.push_str(&input[copied..]);
    result
}

/// The length of the URL authority that `text` starts with: up to any of
/// `/ ? # ' "`, a space, a newline, or the `://` of the next URL in the word.
/// Other whitespace does not end it: a carriage return inside a userinfo
/// (`https://user:se\rcret@host/`) stays inside what is hidden, where a
/// report keeps only the text after it.
fn url_authority_len(text: &str) -> usize {
    let end = text
        .find(['/', '?', '#', '\'', '"', ' ', '\n'])
        .unwrap_or(text.len());
    end - usize::from(text[..end].ends_with(':') && text[end..].starts_with("//"))
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

    /// A line is redacted from its first secret marker, wherever it falls in
    /// the list: with "token=" before "Authorization:", only the later
    /// marker was redacted and the token's secret stayed, and so it did in a
    /// failure's report where a carriage return follows that marker
    /// (`reported_redacted`).
    #[test]
    fn a_diagnostic_is_redacted_from_its_first_secret_marker() {
        assert_eq!(
            redact_diagnostic("token=SECRET11 Authorization:Done."),
            "token=[redacted]"
        );
        assert_eq!(
            reported_redacted(b"token=SECRET10 Authorization:\rDone.\n", redact_diagnostic),
            "token=[redacted]"
        );
        assert_eq!(
            redact_diagnostic("fatal: Password=SECRET12 proxy-authorization: x"),
            "fatal: Password=[redacted]"
        );
        // A marker inside another ends where it does.
        assert_eq!(
            redact_diagnostic("access_token=SECRET13"),
            "access_token=[redacted]"
        );
    }

    /// A credential written with a colon is cut as its `=` form is, from the
    /// first marker on the line and in any ASCII case: `password: hunter2`
    /// once stayed whole, since only `password=` was listed.
    #[test]
    fn a_credential_written_with_a_colon_is_cut_as_its_equals_form_is() {
        assert_eq!(
            redact_diagnostic("password: hunter2"),
            "password:[redacted]"
        );
        for name in [
            "password",
            "passwd",
            "access_token",
            "refresh_token",
            "token",
            "oauth_token",
        ] {
            for name in [name.to_owned(), name.to_ascii_uppercase()] {
                assert_eq!(
                    redact_diagnostic(&format!("fatal: {name}: SECRET after")),
                    format!("fatal: {name}:[redacted]"),
                );
                assert_eq!(
                    redact_diagnostic(&format!("{name}:SECRET")),
                    format!("{name}:[redacted]"),
                );
            }
        }
        // The first marker on the line wins, whichever form it has.
        assert_eq!(
            redact_diagnostic("token: SECRET1 password=SECRET2"),
            "token:[redacted]"
        );
        assert_eq!(
            redact_diagnostic("password=SECRET1 token: SECRET2"),
            "password=[redacted]"
        );
        // Where the secret follows a carriage return, as a failure's report
        // reads it (`reported_redacted`).
        assert_eq!(
            reported_redacted(b"passwd:\rSECRET\n", redact_diagnostic),
            "passwd:[redacted]"
        );
        // A URL's userinfo is the URL stage's to hide, whatever it spells: a
        // token used as the username keeps the host and what follows it.
        assert_eq!(
            redact_diagnostic("https://x-access-token:SECRET@host/repo.git: failed"),
            "https://[redacted]@host/repo.git: failed"
        );
    }

    /// Every credential marker, spelt out so that a marker dropped from the
    /// list fails a test.
    const MARKERS: [&str; 14] = [
        "authorization:",
        "proxy-authorization:",
        "password=",
        "password:",
        "passwd=",
        "passwd:",
        "access_token=",
        "access_token:",
        "refresh_token=",
        "refresh_token:",
        "token=",
        "token:",
        "oauth_token=",
        "oauth_token:",
    ];

    /// A marker with nothing after it on its line names no secret, so the
    /// line is left alone: OpenSSH's `user@host's password: ` read
    /// `user@host's password:[redacted]` in the dialog once `password:` was
    /// a marker. Nothing means only whitespace up to the line's `\n`.
    #[test]
    fn a_credential_marker_with_nothing_after_it_on_its_line_is_left_alone() {
        assert_eq!(
            redact_diagnostic("user@host's password: "),
            "user@host's password: "
        );
        for marker in MARKERS {
            for marker in [marker.to_owned(), marker.to_ascii_uppercase()] {
                for whole in [
                    marker.clone(),
                    format!("{marker} "),
                    format!("fatal: {marker}\t  "),
                    format!("{marker}\r"),
                    format!("{marker} \u{a0}"),
                ] {
                    assert_eq!(redact_diagnostic(&whole), whole);
                }
                // A carriage return does not end the line for this rule.
                for cut in [
                    format!("{marker}\rSECRET"),
                    format!("{marker} \r SECRET"),
                    format!("{marker}\r\rSECRET after"),
                    format!("{marker} SECRET "),
                ] {
                    assert_eq!(redact_diagnostic(&cut), format!("{marker}[redacted]"));
                }
                // A line feed does, so the next line is no value of this one.
                assert_eq!(
                    redact_diagnostic(&format!("{marker} \nnext line")),
                    format!("{marker} \nnext line"),
                );
                assert_eq!(
                    redact_diagnostic(&format!("{marker}\r\ntoken=SECRET")),
                    format!("{marker}\ntoken=[redacted]"),
                );
            }
        }
        // The cut still runs from the first marker on the line with a value,
        // and a later marker is that value, whatever follows it.
        assert_eq!(
            redact_diagnostic("token: SECRET password: "),
            "token:[redacted]"
        );
        assert_eq!(redact_diagnostic("password: token="), "password:[redacted]");
        assert_eq!(
            redact_diagnostic("password: \ntoken: SECRET\npasswd=\t"),
            "password: \ntoken:[redacted]\npasswd=\t"
        );
        // As a failure reads it, the secret after a carriage return is gone
        // and a marker that had none stays as it was.
        assert_eq!(
            reported_redacted(b"password:\rSECRET\n", redact_diagnostic),
            "password:[redacted]"
        );
        assert_eq!(
            reported_redacted(b"password: \r\nDone.\n", redact_diagnostic),
            "password: \nDone."
        );
    }

    /// Every `://` in a whitespace-separated word is a URL of its own: only
    /// the first had its userinfo hidden, and the rest of the word was
    /// shown.
    #[test]
    fn every_url_in_a_word_has_its_userinfo_and_query_hidden() {
        assert_eq!(
            redact_diagnostic("url=https://a:b@one,https://c:d@two/x?y=1"),
            "url=https://[redacted]@one,https://[redacted]@two/x?[redacted]"
        );
        assert_eq!(
            redact_diagnostic("a://u@h1,b://v@h2,c://w@h3/p rest"),
            "a://[redacted]@h1,b://[redacted]@h2,c://[redacted]@h3/p rest"
        );
        assert_eq!(
            redact_diagnostic("https://h/p,https://c:d@two/x rest"),
            "https://h/p,https://[redacted]@two/x rest"
        );
        // A query still hides the word's end, any later URL in it included.
        assert_eq!(
            redact_diagnostic("https://h/p?a=1,https://c:d@two/x rest"),
            "https://h/p?[redacted] rest"
        );
        // One word's URLs do not reach the next word's.
        assert_eq!(
            redact_diagnostic("https://a@one https://b@two"),
            "https://[redacted]@one https://[redacted]@two"
        );
        // A marker in a later URL's userinfo once cut its line, the URL being
        // shown: the line is still cut there, or what followed would return.
        assert_eq!(
            redact_diagnostic("https://a@one,https://c:token=d@two/ rest"),
            "https://[redacted]@one,https://[redacted]"
        );
        // The first URL's userinfo was always hidden whatever it spelt, and
        // never cut the line.
        assert_eq!(
            redact_diagnostic("https://c:token=d@two/ rest"),
            "https://[redacted]@two/ rest"
        );
    }

    /// A carriage return inside userinfo does not end it: a URL's authority
    /// ends at `/ ? # ' "`, a space or a newline, where
    /// `https://user:se\rcret@host/` printed `cret@host`, a report showing
    /// only that much.
    #[test]
    fn a_carriage_return_inside_userinfo_stays_hidden() {
        for (written, shown) in [
            ("https://user:se\rcret@host/", "https://[redacted]@host/"),
            (
                "https://a\r\rb@host/p rest",
                "https://[redacted]@host/p rest",
            ),
            ("https://a\rb@c\rd@host/", "https://[redacted]@host/"),
            ("https://se\tcret@host/", "https://[redacted]@host/"),
            (
                "fatal: 'https://user:se\rcret@host/x': denied",
                "fatal: 'https://[redacted]@host/x': denied",
            ),
            (
                "url=https://a@one,https://b\rc@two/x rest",
                "url=https://[redacted]@one,https://[redacted]@two/x rest",
            ),
            // Each of the characters that end an authority does, before an
            // `@` that is then no userinfo's.
            ("https://a\rb c@host/", "https://a\rb c@host/"),
            ("https://a\nb@host/", "https://a\nb@host/"),
            ("https://a\rb/c@host", "https://a\rb/c@host"),
            ("https://a\rb'c@host", "https://a\rb'c@host"),
            ("https://a\rb\"c@host", "https://a\rb\"c@host"),
            ("https://a\rb?c@host", "https://a\rb?[redacted]"),
            ("https://a\rb#c@host", "https://a\rb?[redacted]"),
            // A host keeps what follows it, a carriage return included.
            ("https://u@host\rnext", "https://[redacted]@host\rnext"),
            // Text around a carriage return outside a URL stays as it was.
            (
                "Receiving 10%\rhttps://u:p@host/",
                "Receiving 10%\rhttps://[redacted]@host/",
            ),
            ("plain\rtext a@b", "plain\rtext a@b"),
            (
                "https://host/path\rother@text",
                "https://host/path\rother@text",
            ),
            ("https://host\rplain words", "https://host\rplain words"),
        ] {
            assert_eq!(redact_diagnostic(written), shown, "{written:?}");
        }
        assert_eq!(
            reported_redacted(b"fatal: https://user:se\rcret@host/\n", redact_diagnostic),
            "fatal: https://[redacted]@host/"
        );
    }

    /// A userinfo that a carriage return runs through hides what used to be
    /// text after a marker, and what follows a marker is never shown: the
    /// line is cut there, or hiding the userinfo would bring back the host
    /// and the rest of the line.
    #[test]
    fn a_marker_in_userinfo_after_a_carriage_return_still_cuts_its_line() {
        for (written, shown) in [
            ("https://h\rtoken=S@h2/x rest", "https://[redacted]"),
            ("https://token=a\rb@d/ rest", "https://[redacted]"),
            ("https://a@b\rtoken=c@d/ rest", "https://[redacted]"),
            ("https://a\rpassword:x@h/ rest", "https://[redacted]"),
            (
                "https://a\rPASSWD=x@h/ rest\nnext",
                "https://[redacted]\nnext",
            ),
            // A space ends the authority, so this marker is the line's own.
            (
                "https://a\rpassword: x@h/",
                "https://a\rpassword:[redacted]",
            ),
            // In the host, the marker cuts as any other text does.
            (
                "https://a@b\rtoken=c",
                "https://[redacted]@b\rtoken=[redacted]",
            ),
            // No carriage return, no cut: a first URL keeps its host.
            (
                "https://c:token=d@two/ rest",
                "https://[redacted]@two/ rest",
            ),
        ] {
            assert_eq!(redact_diagnostic(written), shown, "{written:?}");
        }
    }

    /// What OpenSSH 10.6 and Git 2.56 hand an askpass program, and what the
    /// dialog shows of it. Every prompt reads as its program wrote it, but
    /// for Git's password prompt: it names a URL, whose userinfo is hidden
    /// wherever it appears.
    const PROMPTS: [(&str, &str); 10] = [
        ("user@host's password: ", "user@host's password: "),
        ("(user@host) Password: ", "(user@host) Password: "),
        (
            "Enter user@host's old password: ",
            "Enter user@host's old password: ",
        ),
        (
            "Retype user@host's new password: ",
            "Retype user@host's new password: ",
        ),
        (
            "Enter passphrase for key '/home/u/.ssh/id_ed25519': ",
            "Enter passphrase for key '/home/u/.ssh/id_ed25519': ",
        ),
        (
            "Enter PIN for 'ED25519-SK SHA256:abc': ",
            "Enter PIN for 'ED25519-SK SHA256:abc': ",
        ),
        (
            "Username for 'https://example.com': ",
            "Username for 'https://example.com': ",
        ),
        (
            "Password for 'https://u@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "The authenticity of host 'example.com (192.0.2.1)' can't be established.\nED25519 key fingerprint is: SHA256://b+c\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "The authenticity of host 'example.com (192.0.2.1)' can't be established.\nED25519 key fingerprint is: SHA256://b+c\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ",
        ),
        (
            "Please type 'yes', 'no' or the fingerprint: ",
            "Please type 'yes', 'no' or the fingerprint: ",
        ),
    ];

    /// Git's and OpenSSH's failures, and what a report keeps of them.
    const FAILURES: [(&str, &str); 14] = [
        (
            "fatal: Authentication failed for 'https://example.com/repo.git/'",
            "fatal: Authentication failed for 'https://example.com/repo.git/'",
        ),
        (
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled",
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled",
        ),
        (
            "git@example.com: Permission denied (publickey,password).",
            "git@example.com: Permission denied (publickey,password).",
        ),
        (
            "user@host: Permission denied, please try again.",
            "user@host: Permission denied, please try again.",
        ),
        (
            "Host key verification failed.",
            "Host key verification failed.",
        ),
        (
            "remote: Invalid username or password.",
            "remote: Invalid username or password.",
        ),
        (
            "fatal: Could not read from remote repository.",
            "fatal: Could not read from remote repository.",
        ),
        (
            "fatal: could not read Password for 'https://u@example.com': terminal prompts disabled",
            "fatal: could not read Password for 'https://[redacted]@example.com': terminal prompts disabled",
        ),
        (
            "fatal: unable to access 'https://u:secret@example.com/x?token=abc': The requested URL returned error: 403",
            "fatal: unable to access 'https://[redacted]@example.com/x?[redacted] The requested URL returned error: 403",
        ),
        (
            "fatal: repository 'https://x-access-token:secret@example.com/r.git/' not found",
            "fatal: repository 'https://[redacted]@example.com/r.git/' not found",
        ),
        ("Authorization: Bearer secret", "Authorization:[redacted]"),
        ("remote: password: hunter2", "remote: password:[redacted]"),
        ("remote: token=abc123 for you", "remote: token=[redacted]"),
        (
            "error: helper said access_token: abc",
            "error: helper said access_token:[redacted]",
        ),
    ];

    #[test]
    fn a_prompt_reads_as_written_and_a_failure_loses_only_its_secrets() {
        for (written, shown) in PROMPTS.into_iter().chain(FAILURES) {
            assert_eq!(redact_diagnostic(written), shown, "{written:?}");
        }
    }

    /// The same prompts over the askpass socket, the path by which they
    /// reach the redaction and the dialog.
    #[cfg(unix)]
    #[test]
    fn the_authentication_dialog_shows_each_prompt_as_the_table_says() {
        for (written, shown) in PROMPTS {
            let control = OperationControl::default();
            let server = AskpassServer::new(control.clone()).unwrap();
            let socket = server.socket.clone();
            let helper = thread::spawn(move || askpass_exchange(&socket, written).unwrap());
            let started = Instant::now();
            let prompt = loop {
                if let Some(prompt) = control.take_prompt() {
                    break prompt;
                }
                assert!(started.elapsed() < Duration::from_secs(3), "{written:?}");
                thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(prompt.message, shown, "{written:?}");
            assert!(control.answer(prompt.id, Some("fixture-answer".into())));
            assert_eq!(helper.join().unwrap(), b"fixture-answer");
        }
    }

    /// The redaction as it stood at 1e2ad7da, frozen as the oracle of the
    /// generated diagnostics below: what it hid stays hidden.
    fn earlier_redaction(input: &str) -> String {
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
                [
                    "authorization:",
                    "proxy-authorization:",
                    "password=",
                    "passwd=",
                    "access_token=",
                    "refresh_token=",
                    "token=",
                    "oauth_token=",
                ]
                .into_iter()
                .filter_map(|marker| lower.find(marker).map(|index| index + marker.len()))
                .min()
                .map_or_else(
                    || line.to_owned(),
                    |end| format!("{}[redacted]", &line[..end]),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Seeded generator of diagnostics: every word it writes is unique, so
    /// finding one in an output means that output shows it.
    struct Generated {
        state: u64,
        words: usize,
    }

    impl Generated {
        fn below(&mut self, limit: usize) -> usize {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            (self.state % limit as u64) as usize
        }
        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.below(from.len())]
        }
        fn word(&mut self, kind: &str) -> String {
            self.words += 1;
            format!("{kind}{:07}", self.words)
        }
        fn piece(&mut self) -> String {
            let [secret, secret2, secret3] = [(); 3].map(|()| self.word("SECRET"));
            let [other, other2] = [(); 2].map(|()| self.word("WORD"));
            match self.below(8) {
                0 | 1 => {
                    let marker = self.pick(&[
                        "authorization:",
                        "proxy-authorization:",
                        "password=",
                        "password:",
                        "passwd=",
                        "passwd:",
                        "access_token=",
                        "access_token:",
                        "refresh_token=",
                        "refresh_token:",
                        "token=",
                        "token:",
                        "oauth_token=",
                        "oauth_token:",
                    ]);
                    let marker = if self.below(2) == 0 {
                        marker.to_ascii_uppercase()
                    } else {
                        marker.to_owned()
                    };
                    let gap = self.pick(&["", " ", "\r", " \r"]);
                    format!("{marker}{gap}{secret}")
                }
                2 => format!("https://{secret}:{secret2}@{other}/path"),
                3 => format!("https://{other}/path?{secret}={secret2}"),
                4 => format!("https://{secret}@{other},https://{secret2}@{other2}/x?{secret3}=1"),
                5 => format!("https://{other}/path,https://{secret}:{secret2}@{other2}"),
                6 => {
                    let marker = self.pick(&["token=", "PASSWORD=", "Authorization:"]);
                    format!("https://{other},https://{secret}:{marker}{secret2}@{other2}/path")
                }
                _ => other,
            }
        }
        fn line(&mut self) -> String {
            let mut line = self.pick(&["", "fatal: ", "remote: "]).to_owned();
            for piece in 0..=self.below(5) {
                if piece > 0 {
                    // A query runs to the word's end and takes a marker glued
                    // to it along, which leaves that marker's next word shown
                    // (`?token=x shown`, as ever): only whitespace follows one.
                    let in_query = line
                        .rsplit(char::is_whitespace)
                        .next()
                        .is_some_and(|word| word.contains('?'));
                    let gaps = [" ", "\t", "\r", " \r", "\r ", ","];
                    line.push_str(self.pick(&gaps[..gaps.len() - usize::from(in_query)]));
                }
                let piece = self.piece();
                line.push_str(&piece);
            }
            line
        }
    }

    /// What `redact_diagnostic` hid at 1e2ad7da stays hidden: no generated
    /// word it did not show comes back, its cut on a line without a URL is a
    /// prefix of the earlier cut, and no generated secret is shown at all.
    #[test]
    fn no_generated_diagnostic_shows_what_the_earlier_redaction_hid() {
        const CASES: usize = 20_000;
        let mut generated = Generated {
            state: 0x9E37_79B9_7F4A_7C15,
            words: 0,
        };
        let (mut leaked, mut shown_again, mut longer_cut) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..CASES {
            let first_word = generated.words + 1;
            let mut input = generated.line();
            for _ in 0..generated.below(3) {
                input.push_str(generated.pick(&["\n", "\r\n"]));
                let line = generated.line();
                input.push_str(&line);
            }
            let (earlier, now) = (earlier_redaction(&input), redact_diagnostic(&input));
            if now.contains("SECRET") {
                leaked.push(input.clone());
            }
            let came_back = (first_word..=generated.words).any(|word| {
                ["SECRET", "WORD"].into_iter().any(|kind| {
                    let word = format!("{kind}{word:07}");
                    now.contains(&word) && !earlier.contains(&word)
                })
            });
            if came_back {
                shown_again.push(input.clone());
            }
            let kept = |line: &'_ str| line.strip_suffix("[redacted]").unwrap_or(line).to_owned();
            if earlier.split('\n').zip(now.split('\n')).any(|(old, new)| {
                !old.contains("://") && !new.contains("://") && !kept(old).starts_with(&kept(new))
            }) {
                longer_cut.push(input.clone());
            }
        }
        assert!(
            leaked.is_empty() && shown_again.is_empty() && longer_cut.is_empty(),
            "{CASES} cases: {} show a secret, {} show what the earlier redaction hid, {} cut later; first {:?} {:?} {:?}",
            leaked.len(),
            shown_again.len(),
            longer_cut.len(),
            leaked.first(),
            shown_again.first(),
            longer_cut.first(),
        );
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
    fn askpass_server_waits_for_a_prompt_written_after_accept() {
        use std::os::unix::net::UnixStream;
        let control = OperationControl::default();
        let server = AskpassServer::new(control.clone()).unwrap();
        let mut stream = UnixStream::connect(&server.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        // Outlast the 10 ms accept poll so the server holds the connection before any
        // prompt bytes exist, then split the frame the way a slow helper could.
        let prompt = "Password for 'https://example.invalid/repo': ";
        thread::sleep(Duration::from_millis(100));
        stream
            .write_all(&(prompt.len() as u32).to_be_bytes())
            .unwrap();
        thread::sleep(Duration::from_millis(50));
        stream.write_all(prompt.as_bytes()).unwrap();
        let started = Instant::now();
        let prompt = loop {
            if let Some(prompt) = control.take_prompt() {
                break prompt;
            }
            assert!(started.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(5));
        };
        assert!(control.answer(prompt.id, Some("fixture-answer".into())));
        let mut size = [0; 4];
        stream.read_exact(&mut size).unwrap();
        let mut response = vec![0; u32::from_be_bytes(size) as usize];
        stream.read_exact(&mut response).unwrap();
        assert_eq!(response, b"fixture-answer");
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
        let global_config = directory.path().join("gitconfig");
        std::fs::write(&global_config, "").unwrap();
        run_controlled(OperationControl::default(), || {
            let mut command = normal_command(crate::GitTarget::unconfined(directory.path()));
            command
                .args(["-c", "credential.helper=", "-c"])
                .arg(format!("core.askPass={}", helper.display()))
                .args(["credential", "fill"]);
            configure_askpass(&mut command, true)?;
            // Git prefers GIT_ASKPASS over core.askPass, and system or global
            // settings such as credential.username change the prompts. Clear
            // what a desktop session can export so only this fixture applies.
            command
                .env_remove("GIT_ASKPASS")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_CONFIG_COUNT")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", &global_config);
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
