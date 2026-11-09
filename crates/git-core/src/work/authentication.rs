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
            message: self.redact_prompt(&message),
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
        redact_diagnostic(&self.without_answers(input))
    }
    /// What the authentication dialog shows of `prompt`, its only reader;
    /// progress, failures and diagnostics take [`Self::redact`].
    fn redact_prompt(&self, prompt: &str) -> String {
        shown_prompt(&self.without_answers(prompt))
    }
    fn without_answers(&self, input: &str) -> String {
        let state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut output = input.to_owned();
        for secret in &state.secrets {
            output = output.replace(secret, "[redacted]");
        }
        output
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
    operation().map_err(|error| crate::redacted_failure(&error, |text| control.redact(text)))
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
    route_askpass(command, configured_git_askpass, &executable, |name| {
        std::env::var_os(name)
    });
    Ok(())
}

/// Points Git and SSH at GitTurtle's own prompts, `executable`, unless a
/// program of the user's is theirs: `core.askPass`, or a value in `inherited`,
/// the environment `command` starts from.
///
/// An empty `GIT_ASKPASS` or `SSH_ASKPASS` names no program and counts as
/// unset, as Git's `if (askpass && *askpass)` reads it: kept, a Push from a
/// shell that exports them empty failed at once with no dialog. Git reads an
/// empty `GIT_ASKPASS` before `core.askPass` and stops there, so where that
/// setting is configured the empty value is taken away for Git to reach it.
/// An empty `SSH_ASKPASS_REQUIRE` names no policy, since OpenSSH reads only
/// `force`, `prefer` and `never`, so SSH is told to use the prompts as where
/// it is unset: kept, a desktop launch with no display left SSH no prompt
/// and a Push failed with no dialog. A policy of the user's stays theirs.
fn route_askpass(
    command: &mut Command,
    configured_git_askpass: bool,
    executable: &Path,
    inherited: impl Fn(&str) -> Option<std::ffi::OsString>,
) {
    // An empty value is no value, a program or a policy alike.
    let set = |name| inherited(name).is_some_and(|value| !value.is_empty());
    if !set("GIT_ASKPASS") {
        if configured_git_askpass {
            command.env_remove("GIT_ASKPASS");
        } else {
            command.env("GIT_ASKPASS", executable);
        }
    }
    if !set("SSH_ASKPASS") {
        command.env("SSH_ASKPASS", executable);
    }
    // Askpass supplies the UI in a GUI process with no controlling terminal.
    // Existing programs and host-verification policy remain in charge.
    if !set("SSH_ASKPASS_REQUIRE") {
        command.env("SSH_ASKPASS_REQUIRE", "force");
    }
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
/// runs to the word's end and so hides any later URL in it. A marker hidden
/// with a later URL's userinfo, or with a userinfo or query reached through
/// whitespace in the authority, cuts the rest of its line: it was text on
/// that line, and what follows a marker is never shown.
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
        let line_end = || input[end..].find('\n').map_or(input.len(), |i| end + i);
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
                copied = line_end();
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
            // A query that whitespace in the authority puts past the word
            // would likewise take a marker away from the line's cut.
            copied = if authority.contains(char::is_whitespace)
                && first_credential_marker_end(&rest[query..]).is_some()
            {
                line_end()
            } else {
                word_end
            };
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

/// `prompt` as the dialog shows it: as [`redact_diagnostic`] reads it, with
/// the username restored in Git's own password prompt, so a person can tell
/// which account is being asked for. Nothing else differs.
fn shown_prompt(prompt: &str) -> String {
    let shown = redact_diagnostic(prompt);
    match git_prompt_username(prompt) {
        Some(name) => shown.replacen("://[redacted]@", &format!("://{name}@"), 1),
        None => shown,
    }
}

/// The username `prompt` names, if `prompt` is exactly what Git asks,
/// `Password for '<url>': `, for one URL on one line, and its userinfo reads
/// as an account name.
fn git_prompt_username(prompt: &str) -> Option<&str> {
    let url = prompt.strip_prefix("Password for '")?.strip_suffix("': ")?;
    if url.contains(|c: char| c.is_whitespace() || c.is_control() || c == '\'' || c == '"') {
        return None;
    }
    let (_, rest) = url.split_once("://")?;
    if rest.contains("://") {
        return None;
    }
    let (name, _) = rest.split(['/', '?', '#']).next()?.rsplit_once('@')?;
    reads_as_account_name(name).then_some(name)
}

/// Whether `name`, a whole userinfo, reads as an account name. What does not
/// stays hidden: a second part (`user:secret`) and every credential marker,
/// each of which holds a `:` or an `=`, written as Git prints them with its
/// default `credential.sanitizePrompt`, `%3A` and `%3D`, or literally with it
/// off, and a token, which Git asks with in the username's place when a URL
/// holds one in it (`https://ghp_…@github.com`): one its issuer's prefix
/// names, or whose words, or letters and digits taken together, read as
/// random text.
fn reads_as_account_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let parts = with_email_separators(name);
    let joined = parts
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .collect::<Vec<_>>();
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-+@%".contains(&byte))
        && !["%3a", "%3d"].iter().any(|encoded| lower.contains(encoded))
        && !has_issuer_prefix(name)
        && parts.split(['.', '_', '-', '+', '@']).all(reads_as_a_word)
        && (joined.len() < 20 || !reads_as_random(&joined))
}

/// `name` with each `%40`, as Git prints an email's `@`, written as the `@`
/// it separates by: only where the part after it holds a `.`, as a domain
/// does and a percent-encoded password need not.
fn with_email_separators(name: &str) -> String {
    let mut parts = name.split("%40");
    let mut written = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        written.push_str(if part.contains('.') { "@" } else { "%40" });
        written.push_str(part);
    }
    written
}

/// Whether `name` starts as an issuer writes its tokens, which a separator
/// further on cannot make a name: GitHub's `ghp_` family and `github_pat_`,
/// GitLab's `glpat-`, `gldt-` and `glrt-`, and Docker's `dckr_pat_`.
fn has_issuer_prefix(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["github_pat_", "glpat-", "gldt-", "glrt-", "dckr_pat_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || matches!(name.as_bytes(), [b'g', b'h', letter, b'_', ..] if letter.is_ascii_lowercase())
}

/// Whether `word`, a run between a name's separators, reads as part of a
/// name. Any word under 12 bytes does. From there it must not read as random
/// text, and no name has an unbroken run of 20 bytes. A token's separators
/// can cut it into words that each pass, so a word passing proves nothing of
/// the whole name, which [`reads_as_account_name`] judges joined as well.
fn reads_as_a_word(word: &str) -> bool {
    word.len() < 12 || (word.len() < 20 && !reads_as_random(word.as_bytes()))
}

/// Whether `bytes` read as random text rather than a name, whose digits
/// trail its letters and whose case changes rarely.
fn reads_as_random(bytes: &[u8]) -> bool {
    let letter_after_digit = bytes
        .iter()
        .skip_while(|byte| !byte.is_ascii_digit())
        .any(u8::is_ascii_alphabetic);
    let case_changes = bytes
        .windows(2)
        .filter(|pair| pair[0].is_ascii_uppercase() != pair[1].is_ascii_uppercase())
        .count();
    letter_after_digit || case_changes * 3 > bytes.len()
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

    /// A query reached across whitespace in its authority was text on the
    /// line, and what followed a marker in it was never shown: hiding the
    /// query takes that marker away from the line's cut, so the line is cut
    /// here too.
    #[test]
    fn a_marker_in_a_query_after_whitespace_in_its_authority_still_cuts_its_line() {
        for (written, shown) in [
            (
                "https://host\r?token=SECRET visible-more",
                "https://host\r?[redacted]",
            ),
            (
                "fatal: unable to access 'https://example.com\t?password=correct horse staple'",
                "fatal: unable to access 'https://example.com\t?[redacted]",
            ),
            (
                "https://u\rv@h/p?access_token=A second-word",
                "https://[redacted]@h/p?[redacted]",
            ),
            (
                "https://u:se\rcret@host/x?token=abc tail",
                "https://[redacted]@host/x?[redacted]",
            ),
            // A fragment is hidden as a query is.
            (
                "https://host\r#token=SECRET visible-more",
                "https://host\r?[redacted]",
            ),
            (
                "https://u\rv@h/p#Authorization:x tail",
                "https://[redacted]@h/p?[redacted]",
            ),
            // Anywhere in the query, and in the colon form.
            ("https://h\r?a=b&token=c rest", "https://h\r?[redacted]"),
            ("https://h\r?token: c rest", "https://h\r?[redacted]"),
            // A later URL in the word, and an authority with the whitespace
            // only after its `@`.
            (
                "https://a,https://b\rc?token=x rest",
                "https://a,https://b\rc?[redacted]",
            ),
            (
                "https://u@h\rx?token=y rest",
                "https://[redacted]@h\rx?[redacted]",
            ),
            // The cut ends with the line.
            (
                "https://host\r?token=a b\nnext line",
                "https://host\r?[redacted]\nnext line",
            ),
            (
                "https://host\r?token=a b\r\nnext line",
                "https://host\r?[redacted]\nnext line",
            ),
            // With no marker in the query only the word is hidden, and with
            // no whitespace in the authority a query hides its word, as ever.
            (
                "https://host\r?a=b visible",
                "https://host\r?[redacted] visible",
            ),
            ("https://h/p?token=x shown", "https://h/p?[redacted] shown"),
            (
                "https://u@h/p#token=x shown",
                "https://[redacted]@h/p?[redacted] shown",
            ),
        ] {
            assert_eq!(redact_diagnostic(written), shown, "{written:?}");
        }
        assert_eq!(
            reported_redacted(
                b"https://host\r?token=SECRET visible-more\n",
                redact_diagnostic
            ),
            "?[redacted]"
        );
    }

    /// What OpenSSH 10.6 and Git 2.56 hand an askpass program, and what the
    /// dialog shows of it. Every prompt reads as its program wrote it: Git's
    /// password prompt names its account, so the person can recognise it,
    /// unless what stands there is not a plain username (the rows below it).
    const PROMPTS: [(&str, &str); 56] = [
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
        // Git's password prompt shows the one username it names.
        (
            "Password for 'https://u@example.com': ",
            "Password for 'https://u@example.com': ",
        ),
        (
            "Password for 'https://git@github.com': ",
            "Password for 'https://git@github.com': ",
        ),
        (
            "Password for 'https://x-access-token@github.com': ",
            "Password for 'https://x-access-token@github.com': ",
        ),
        (
            "Password for 'https://fernando.ramirez@example.com/org/repo.git': ",
            "Password for 'https://fernando.ramirez@example.com/org/repo.git': ",
        ),
        // An email as a username, whose `@` Git prints as `%40`.
        (
            "Password for 'https://fernando.ramirez%40example.com@git.example.com': ",
            "Password for 'https://fernando.ramirez%40example.com@git.example.com': ",
        ),
        // And as Git prints it with credential.sanitizePrompt=false.
        (
            "Password for 'https://me@example.org@git.example.com': ",
            "Password for 'https://me@example.org@git.example.com': ",
        ),
        (
            "Password for 'https://Jos%C3%A9@example.com:8443': ",
            "Password for 'https://Jos%C3%A9@example.com:8443': ",
        ),
        (
            "Password for 'https://fernando1987@example.com': ",
            "Password for 'https://fernando1987@example.com': ",
        ),
        (
            "Password for 'https://FernandoRamirez@example.com': ",
            "Password for 'https://FernandoRamirez@example.com': ",
        ),
        (
            "Password for 'https://j0hn@example.com': ",
            "Password for 'https://j0hn@example.com': ",
        ),
        (
            "Password for 'https://first.last@example.com': ",
            "Password for 'https://first.last@example.com': ",
        ),
        (
            "Password for 'https://me%40corp.com@example.com': ",
            "Password for 'https://me%40corp.com@example.com': ",
        ),
        (
            "Password for 'https://user-at-123456789012@example.com': ",
            "Password for 'https://user-at-123456789012@example.com': ",
        ),
        (
            "Password for 'https://fernando.ramirez.contractor@example.com': ",
            "Password for 'https://fernando.ramirez.contractor@example.com': ",
        ),
        // A second part stays hidden, and with it the first: a colon says
        // this is not one username.
        (
            "Password for 'https://user:secret@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // A credential marker, however it is spelt.
        (
            "Password for 'https://token=abc@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://password:hunter2@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // Those two as Git prints them by default (credential.sanitizePrompt).
        (
            "Password for 'https://token%3Dabc@host.example/': ",
            "Password for 'https://[redacted]@host.example/': ",
        ),
        (
            "Password for 'https://u%3Asecret1@host.example': ",
            "Password for 'https://[redacted]@host.example': ",
        ),
        (
            "Password for 'https://token%3dabc@host.example': ",
            "Password for 'https://[redacted]@host.example': ",
        ),
        (
            "Password for 'https://u%3asecret1@host.example': ",
            "Password for 'https://[redacted]@host.example': ",
        ),
        // A token in the username's place.
        (
            "Password for 'https://ghp_16C7e42F292c6912E7710c838347Ae178B4a@github.com': ",
            "Password for 'https://[redacted]@github.com': ",
        ),
        (
            "Password for 'https://github_pat_11ABCDE0Y0abcdefghijkl_mnopqrstuvwxyzABCDEFGH@github.com': ",
            "Password for 'https://[redacted]@github.com': ",
        ),
        (
            "Password for 'https://glpat-xY7zQ2mN8pR4tL6v@gitlab.com': ",
            "Password for 'https://[redacted]@gitlab.com': ",
        ),
        (
            "Password for 'https://e72e16c7e42f292c6912e7710c838347ae178b4a@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://AKIAIOSFODNN7EXAMPLE@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://f47ac10b-58cc-4372-a567-0e02b2c3d479@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // A token an allowed separator splits is still one, whether its
        // issuer's prefix or its letters and digits together give it away.
        (
            "Password for 'https://glpat-xY7z-Q2mN8pR4_tL6v@gitlab.com/': ",
            "Password for 'https://[redacted]@gitlab.com/': ",
        ),
        (
            "Password for 'https://gldt-abc@gitlab.com': ",
            "Password for 'https://[redacted]@gitlab.com': ",
        ),
        (
            "Password for 'https://glrt-abc@gitlab.com': ",
            "Password for 'https://[redacted]@gitlab.com': ",
        ),
        (
            "Password for 'https://dckr_pat_abc@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://ghs_abc@github.com': ",
            "Password for 'https://[redacted]@github.com': ",
        ),
        (
            "Password for 'https://GHP_abc@github.com': ",
            "Password for 'https://[redacted]@github.com': ",
        ),
        (
            "Password for 'https://github_pat_abc@github.com': ",
            "Password for 'https://[redacted]@github.com': ",
        ),
        (
            "Password for 'https://xY7z-Q2mN8pR4_tL6v-Ab3D@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // `%40` separates an email's parts, each still judged as a word.
        (
            "Password for 'https://k7gNU3sdo4T9%40example.com@git.example.com': ",
            "Password for 'https://[redacted]@git.example.com': ",
        ),
        // A `%40` with no `.` after it is no email's: a password can hold one.
        (
            "Password for 'https://Tr0ub4dor%403xY@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // Shorter, but with digits through its letters or case changing
        // throughout.
        (
            "Password for 'https://K7gNU3sdo4T9@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://KjHgFdSaQwErTyUi@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // No userinfo to show, or not one name.
        (
            "Password for 'https://@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://a\tb@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://aaaaaaaaaaa.aaaaaaaaaaa.aaaaaaaaaaa.aaaaaaaaaaa.aaaaaaaaaaa.aaaaaaaaaaa@example.com': ",
            "Password for 'https://[redacted]@example.com': ",
        ),
        // Nothing but Git's own prompt, whole, shows a username.
        (
            "Username for 'https://u@example.com': ",
            "Username for 'https://[redacted]@example.com': ",
        ),
        (
            "password for 'https://u@example.com': ",
            "password for 'https://[redacted]@example.com': ",
        ),
        (
            "Password for 'https://u@example.com': more",
            "Password for 'https://[redacted]@example.com': more",
        ),
        (
            "Password for 'https://u@one.example,https://v@two.example': ",
            "Password for 'https://[redacted]@one.example,https://[redacted]@two.example': ",
        ),
        (
            "Note\nPassword for 'https://u@example.com': ",
            "Note\nPassword for 'https://[redacted]@example.com': ",
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
        for (written, shown) in PROMPTS {
            assert_eq!(shown_prompt(written), shown, "{written:?}");
        }
        for (written, shown) in FAILURES {
            assert_eq!(redact_diagnostic(written), shown, "{written:?}");
        }
    }

    /// Only the dialog's prompt shows the username: the same words reaching
    /// a diagnostic, a failure or progress keep it hidden, and an account
    /// name equal to an answer already given is hidden in the prompt too.
    #[test]
    fn only_the_dialogs_prompt_shows_a_username() {
        let prompt = "Password for 'https://alice@example.com': ";
        let hidden = "Password for 'https://[redacted]@example.com': ";
        let control = OperationControl::default();
        assert_eq!(control.redact_prompt(prompt), prompt);
        assert_eq!(control.redact(prompt), hidden);
        assert_eq!(redact_diagnostic(prompt), hidden);
        assert_eq!(redact_current(prompt), hidden);
        control
            .0
            .state
            .lock()
            .unwrap()
            .secrets
            .push("alice".to_owned());
        assert_eq!(control.redact_prompt(prompt), hidden);
    }

    /// The dialog's reading of any text differs from a diagnostic's only
    /// where the text is Git's own prompt for one account name, and then
    /// only by that name: so what `no_generated_diagnostic_shows_what_the_earlier_redaction_hid`
    /// holds of a diagnostic holds of a prompt, but for that one username.
    #[test]
    fn no_generated_prompt_shows_more_than_its_username() {
        const CASES: usize = 20_000;
        let mut generated = Generated {
            state: 0x2545_F491_4F6C_DD1D,
            words: 0,
        };
        let mut shown_names = 0;
        for _ in 0..CASES {
            let line = generated.line();
            let name = generated.word("name");
            for written in [
                line.clone(),
                format!("Password for '{line}': "),
                format!("Password for 'https://{line}@example.com': "),
                format!("Password for 'https://{name}@example.com/{line}': "),
                format!("Username for 'https://{name}@example.com': "),
            ] {
                let (diagnostic, prompt) = (redact_diagnostic(&written), shown_prompt(&written));
                let Some(username) = git_prompt_username(&written) else {
                    assert_eq!(prompt, diagnostic, "{written:?}");
                    continue;
                };
                shown_names += 1;
                assert!(
                    !username.contains(':') && reads_as_account_name(username),
                    "{written:?}"
                );
                assert_eq!(
                    prompt,
                    diagnostic.replacen("[redacted]", username, 1),
                    "{written:?}"
                );
            }
        }
        assert!(shown_names > 0, "no generated prompt named an account");
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
            let [other, other2, other3] = [(); 3].map(|()| self.word("WORD"));
            let (space, query) = (self.pick(&["\r", "\t"]), self.pick(&["?", "#"]));
            match self.below(18) {
                0 | 1 => {
                    let marker = self.pick(&MARKERS);
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
                // Whitespace inside a userinfo, and a marker on either side
                // of it.
                7 => format!("https://{secret}\r{secret2}@{other}/path"),
                8 => format!("https://{secret}:{secret2}\t{secret3}@{other}"),
                9 => {
                    let marker = self.pick(&MARKERS);
                    format!("https://{other}\r{marker}{secret}@{other2}/path")
                }
                10 => {
                    let marker = self.pick(&MARKERS);
                    format!("https://{marker}{secret}\r{other}@{other2}/path")
                }
                // A carriage return in a URL with no userinfo, and an `@`
                // outside any URL.
                11 => format!("https://{other}\r{other2}"),
                12 => format!("{other}@{other2}"),
                // A marker in a query reached through whitespace in the
                // authority, with words after that query's own.
                13 => {
                    let marker = self.pick(&MARKERS);
                    format!("https://{other}{space}{query}{marker}{secret} {other2}")
                }
                14 => {
                    let marker = self.pick(&MARKERS);
                    format!("https://{other}{space}{other2}/p{query}{marker}{secret} {other3}")
                }
                15 => {
                    let marker = self.pick(&MARKERS);
                    format!(
                        "https://{secret}{space}{secret2}@{other}/p{query}{marker}{secret3} {other2}"
                    )
                }
                // With no whitespace there, a query hides its word as ever.
                16 => {
                    let marker = self.pick(&MARKERS);
                    format!("https://{other}/p{query}{marker}{secret} {other2}")
                }
                _ => other,
            }
        }
        fn gap(&mut self, line: &str) -> &'static str {
            // A query runs to the word's end and takes a marker glued to it
            // along, which leaves that marker's next word shown (`?token=x
            // shown`, as ever): only whitespace follows one.
            let in_query = line
                .rsplit(char::is_whitespace)
                .next()
                .is_some_and(|word| word.contains('?'));
            let gaps = [" ", "\t", "\r", " \r", "\r ", ","];
            self.pick(&gaps[..gaps.len() - usize::from(in_query)])
        }
        fn line(&mut self) -> String {
            let mut line = self.pick(&["", "fatal: ", "remote: "]).to_owned();
            for piece in 0..=self.below(5) {
                if piece > 0 {
                    line.push_str(self.gap(&line));
                }
                let piece = self.piece();
                line.push_str(&piece);
            }
            // A marker with nothing after it on the line.
            if self.below(4) == 0 {
                line.push_str(self.gap(&line));
                line.push_str(self.pick(&MARKERS));
                line.push_str(self.pick(&["", " ", "\t ", "\r", " \r "]));
            }
            line
        }
    }

    /// `new` as the property reads it against `old`: where `old` cut this
    /// line at a marker that has only whitespace after it, `new` shows that
    /// whitespace, which holds no secret. This is the one exception to
    /// showing nothing `old` hid.
    fn without_a_markers_whitespace<'a>(old: &str, new: &'a str) -> &'a str {
        if old.ends_with("[redacted]") {
            new.trim_end()
        } else {
            new
        }
    }

    /// Whether `new` shows only bytes that `old` shows, in their order and
    /// with the redactions' own words aside.
    fn shows_only_what_it_showed(old: &str, new: &str) -> bool {
        let plain = |line: &str| line.replace("?[redacted]", "").replace("[redacted]", "");
        let (old_shown, new_shown) = (plain(old), plain(without_a_markers_whitespace(old, new)));
        let mut old_shown = old_shown.chars();
        new_shown
            .chars()
            .all(|byte| old_shown.any(|shown| shown == byte))
    }

    /// What `redact_diagnostic` hid at 1e2ad7da stays hidden: on no line does
    /// it show a byte the earlier redaction did not, no generated word it
    /// did not show comes back, its cut on a line without a URL is a prefix
    /// of the earlier cut, and no generated secret is shown at all. The one
    /// exception is the whitespace after a marker with nothing else after it
    /// (`without_a_markers_whitespace`).
    #[test]
    fn no_generated_diagnostic_shows_what_the_earlier_redaction_hid() {
        const CASES: usize = 20_000;
        let mut generated = Generated {
            state: 0x9E37_79B9_7F4A_7C15,
            words: 0,
        };
        let (mut leaked, mut shown_again, mut longer_cut, mut bytes_back) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut left_alone = 0;
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
            let lines = || earlier.split('\n').zip(now.split('\n'));
            if lines().any(|(old, new)| {
                let new = without_a_markers_whitespace(old, new);
                !old.contains("://") && !new.contains("://") && !kept(old).starts_with(&kept(new))
            }) {
                longer_cut.push(input.clone());
            }
            if earlier.split('\n').count() != now.split('\n').count()
                || lines().any(|(old, new)| !shows_only_what_it_showed(old, new))
            {
                bytes_back.push(input.clone());
            }
            left_alone += lines()
                .filter(|(old, new)| old.ends_with("[redacted]") && !new.ends_with("[redacted]"))
                .count();
        }
        assert!(
            leaked.is_empty()
                && shown_again.is_empty()
                && longer_cut.is_empty()
                && bytes_back.is_empty(),
            "{CASES} cases: {} show a secret, {} show a word the earlier redaction hid, {} cut later, {} show a byte it hid; first {:?} {:?} {:?} {:?}",
            leaked.len(),
            shown_again.len(),
            longer_cut.len(),
            bytes_back.len(),
            leaked.first(),
            shown_again.first(),
            longer_cut.first(),
            bytes_back.first(),
        );
        assert!(left_alone > 0, "no generated line has a marker left alone");
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
        assert_eq!(
            prompt.message,
            "Password for 'https://fixture@example.invalid/repo': "
        );
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

    /// `command` run as a controlled operation's own failing step, off the
    /// operation's thread, where nothing knows what the operation answered
    /// until `run_controlled` does: its redaction is the one that names it.
    #[cfg(unix)]
    fn controlled_failure(control: OperationControl, command: Command) -> anyhow::Error {
        run_controlled(control, move || -> Result<()> {
            thread::spawn(move || {
                super::super::checked_write_output(command, None, Duration::from_secs(10)).map(drop)
            })
            .join()
            .unwrap()
        })
        .expect_err("the fake command fails")
    }

    /// A control that has answered `secret`.
    #[cfg(unix)]
    fn answered(secret: &str) -> OperationControl {
        let control = OperationControl::default();
        control
            .0
            .state
            .lock()
            .unwrap()
            .secrets
            .push(secret.to_owned());
        control
    }

    /// A controlled write's failure stays typed, its line whole for the
    /// banner: `run_controlled` once flattened it to its report, so no write
    /// carried a line and an SSH refusal read "Git operation failed:
    /// Permission denied (publickey).", its `fatal:` line lost. Report and
    /// line are each redacted with what the operation answered, and a failure
    /// with no line stays a report.
    #[cfg(unix)]
    #[test]
    fn a_controlled_failure_keeps_its_line_and_hides_what_its_report_hides() {
        for (case, stderr, line) in [
            (
                "the SSH shape",
                "Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
                "fatal: Could not read from remote repository.",
            ),
            (
                "a rejected push",
                "To /x\n ! [rejected]        main -> main (non-fast-forward)\nerror: failed to push some refs to '/x'\nhint: Updates were rejected\n",
                "error: failed to push some refs to '/x'",
            ),
            (
                "a credential URL",
                "hint: first\nfatal: unable to access 'https://user:url-s3cret@example.invalid/x.git/': 403\n",
                "fatal: unable to access 'https://[redacted]@example.invalid/x.git/': 403",
            ),
            (
                "an answered secret",
                "hint: first\nfatal: the server refused answered-s3cret\n",
                "fatal: the server refused [redacted]",
            ),
        ] {
            let mut command = Command::new("sh");
            command
                .args(["-c", "printf %s \"$0\" >&2; exit 1"])
                .arg(stderr);
            let error = controlled_failure(answered("answered-s3cret"), command);
            let report = format!("{error:#}");
            assert_eq!(
                crate::failure_summary(&error).as_deref(),
                Some(line),
                "{case}: {report}"
            );
            for hidden in ["answered-s3cret", "url-s3cret"] {
                assert!(!report.contains(hidden), "{case}: {report}");
            }
        }
        let untyped = run_controlled(answered("answered-s3cret"), || -> Result<()> {
            bail!("Original-series save failed: answered-s3cret")
        })
        .expect_err("the operation fails");
        assert_eq!(
            format!("{untyped:#}"),
            "Original-series save failed: [redacted]"
        );
        assert_eq!(crate::failure_summary(&untyped), None);
    }

    /// Git's words for a folder with no repository stay typed through
    /// `run_controlled`, the failure's line as hidden as any other's.
    #[cfg(unix)]
    #[test]
    fn a_controlled_failure_stays_not_a_repository() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf 'fatal: not a git repository: /tmp/answered-s3cret\\n' >&2; exit 128",
        ]);
        let error = controlled_failure(answered("answered-s3cret"), command);
        assert!(
            error
                .chain()
                .any(|cause| cause.is::<crate::NotARepository>()),
            "{error:#}"
        );
        assert_eq!(
            crate::failure_summary(&error).as_deref(),
            Some("fatal: not a git repository: /tmp/[redacted]"),
        );
        assert!(
            !format!("{error:#}").contains("answered-s3cret"),
            "{error:#}"
        );
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

    /// Which program answers Git's prompts, and which SSH is told to run and
    /// when, for each state a launching shell can leave the three variables
    /// in: real Git runs the prompts, from the environment a process would
    /// have inherited. An empty value is no program, so the app's prompts are
    /// used, or `core.askPass` where it is configured; a program in either
    /// still wins, each variable read on its own. An empty
    /// `SSH_ASKPASS_REQUIRE` is no policy, as OpenSSH reads it, so SSH is
    /// told to use its askpass as where the variable is unset: kept, it left
    /// SSH with no prompt in a desktop launch with no display. A policy of
    /// the user's, `never` or `prefer`, stays theirs.
    #[cfg(unix)]
    #[test]
    fn an_empty_askpass_in_the_environment_counts_as_unset() {
        use std::ffi::{OsStr, OsString};
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let program = |name: &str| {
            let path = directory.path().join(name);
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\ncase \"$1\" in Username*) printf '{name}-user' ;; \
                     *) printf '{name}-password' ;; esac\n"
                ),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path
        };
        let (app, theirs, configured) = (program("app"), program("theirs"), program("configured"));
        let global_config = directory.path().join("gitconfig");
        std::fs::write(&global_config, "").unwrap();
        let empty = || Some(OsString::new());
        let named = || Some(theirs.clone().into_os_string());
        let policy = |policy: &str| Some(OsString::from(policy));
        // What `GIT_ASKPASS`, `SSH_ASKPASS` and `SSH_ASKPASS_REQUIRE` hold,
        // whether `core.askPass` is set, who answers Git, what SSH is told to
        // run and when.
        let cases = [
            (
                "both empty",
                [empty(), empty(), None],
                false,
                "app",
                &app,
                "force",
            ),
            (
                "both unset",
                [None, None, None],
                false,
                "app",
                &app,
                "force",
            ),
            (
                "both empty beside core.askPass",
                [empty(), empty(), None],
                true,
                "configured",
                &app,
                "force",
            ),
            (
                "both unset beside core.askPass",
                [None, None, None],
                true,
                "configured",
                &app,
                "force",
            ),
            (
                "both name a program",
                [named(), named(), None],
                false,
                "theirs",
                &theirs,
                "force",
            ),
            (
                "both name a program beside core.askPass",
                [named(), named(), None],
                true,
                "theirs",
                &theirs,
                "force",
            ),
            (
                "GIT_ASKPASS empty, SSH_ASKPASS a program",
                [empty(), named(), None],
                false,
                "app",
                &theirs,
                "force",
            ),
            (
                "GIT_ASKPASS empty, SSH_ASKPASS a program, beside core.askPass",
                [empty(), named(), None],
                true,
                "configured",
                &theirs,
                "force",
            ),
            (
                "GIT_ASKPASS a program, SSH_ASKPASS empty",
                [named(), empty(), None],
                false,
                "theirs",
                &app,
                "force",
            ),
            (
                "GIT_ASKPASS a program, SSH_ASKPASS empty, beside core.askPass",
                [named(), empty(), None],
                true,
                "theirs",
                &app,
                "force",
            ),
            (
                "SSH_ASKPASS_REQUIRE empty",
                [None, None, empty()],
                false,
                "app",
                &app,
                "force",
            ),
            (
                "SSH_ASKPASS_REQUIRE never",
                [None, None, policy("never")],
                false,
                "app",
                &app,
                "never",
            ),
            (
                "SSH_ASKPASS_REQUIRE prefer",
                [empty(), empty(), policy("prefer")],
                false,
                "app",
                &app,
                "prefer",
            ),
        ];
        for (at, values, core_askpass, answers, ssh, required) in cases {
            let inherited = ["GIT_ASKPASS", "SSH_ASKPASS", "SSH_ASKPASS_REQUIRE"]
                .into_iter()
                .zip(values)
                .collect::<Vec<_>>();
            let told = run_controlled(OperationControl::default(), || {
                let mut command = normal_command(crate::GitTarget::unconfined(directory.path()));
                command.args(["-c", "credential.helper="]);
                if core_askpass {
                    command
                        .arg("-c")
                        .arg(format!("core.askPass={}", configured.display()));
                }
                command
                    .args(["credential", "fill"])
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .env_remove("GIT_CONFIG_PARAMETERS")
                    .env_remove("GIT_CONFIG_COUNT")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", &global_config);
                for (name, value) in &inherited {
                    match value {
                        Some(value) => command.env(name, value),
                        None => command.env_remove(name),
                    };
                }
                route_askpass(&mut command, core_askpass, &app, |name| {
                    inherited
                        .iter()
                        .find(|(variable, _)| *variable == name)
                        .and_then(|(_, value)| value.clone())
                });
                let told = |name| {
                    command
                        .get_envs()
                        .find(|(variable, _)| *variable == OsStr::new(name))
                        .and_then(|(_, value)| value.map(OsString::from))
                };
                let told = (told("SSH_ASKPASS"), told("SSH_ASKPASS_REQUIRE"));
                let output = bounded_write_output(
                    command,
                    Some(b"protocol=https\nhost=fixture.invalid\n\n".to_vec()),
                    Duration::from_secs(3),
                )?;
                ensure!(
                    output.status.success(),
                    "{at}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let values = String::from_utf8(output.stdout)?;
                ensure!(
                    values.contains(&format!("username={answers}-user"))
                        && values.contains(&format!("password={answers}-password")),
                    "{at}: {answers} did not answer: {values}"
                );
                Ok(told)
            })
            .unwrap();
            assert_eq!(
                told.0.as_deref(),
                Some(ssh.as_os_str()),
                "{at}: what SSH is told to run"
            );
            assert_eq!(
                told.1.as_deref(),
                Some(OsStr::new(required)),
                "{at}: when SSH is told to run it"
            );
        }
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
