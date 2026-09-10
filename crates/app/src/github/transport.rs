//! Supported GitHub CLI OAuth handoff, native credential storage, and bounded API
//! subprocesses. No token appears in an argument, preference, log, or diagnostic.
use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
const SERVICE: &str = "com.gitturtle.github.oauth";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "github.com";
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Credential {
    pub login: String,
    token: String,
}
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("login", &self.login)
            .finish_non_exhaustive()
    }
}
/// Platform abstraction never falls back to ordinary files. Linux can add a
/// Secret Service implementation without changing collaboration semantics.
pub(crate) trait CredentialStore {
    fn load(&self) -> Result<Option<Credential>>;
    fn save(&self, credential: &Credential) -> Result<()>;
    fn remove(&self) -> Result<()>;
}
pub(crate) struct SecureStore;
#[cfg(target_os = "macos")]
impl CredentialStore for SecureStore {
    fn load(&self) -> Result<Option<Credential>> {
        match security_framework::passwords::get_generic_password(SERVICE, ACCOUNT) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
                anyhow!("The GitHub Keychain entry is invalid. Connect the account again.")
            })?)),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(_) => bail!(
                "GitHub credential access was refused by Keychain. Unlock the login Keychain or reconnect explicitly."
            ),
        }
    }
    fn save(&self, credential: &Credential) -> Result<()> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            ACCOUNT,
            &serde_json::to_vec(credential)?,
        )
        .map_err(|_| {
            anyhow!(
                "Keychain could not save GitHub authorization. No plaintext credential was stored."
            )
        })
    }
    fn remove(&self) -> Result<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(_) => bail!("Keychain could not remove the GitHub authorization"),
        }
    }
}
#[cfg(not(target_os = "macos"))]
impl CredentialStore for SecureStore {
    fn load(&self) -> Result<Option<Credential>> {
        Ok(None)
    }
    fn save(&self, _: &Credential) -> Result<()> {
        bail!(
            "Secure GitHub credential storage is not yet available on this platform. No plaintext credential was stored."
        )
    }
    fn remove(&self) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct GhTransport {
    credential: Credential,
}
impl GhTransport {
    pub fn stored() -> Result<Self> {
        Ok(Self { credential:SecureStore.load()?.ok_or_else(||anyhow!("Connect a GitHub account first. Sign in through GitHub CLI using ‘gh auth login --hostname github.com --web --skip-ssh-key’, then choose Connect GitHub CLI account."))? })
    }
    pub fn login(&self) -> &str {
        &self.credential.login
    }
    /// This is an explicit account connection, including the /user request.
    /// GitHub CLI owns browser OAuth. GitTurtle never invokes auth setup-git,
    /// switches the CLI account, uploads a key, or creates a credential file.
    pub fn connect_existing(control: &OperationControl) -> Result<String> {
        let mut command = gh_command()?;
        command.args(["auth", "token", "--hostname", "github.com"]);
        let (success, token)=run(command,None,control,16*1024,Duration::from_secs(30)).map_err(|_|anyhow!("GitHub CLI has no usable signed-in account. In Terminal run ‘gh auth login --hostname github.com --web --skip-ssh-key’, then choose Connect GitHub CLI account. Never paste the token here."))?;
        ensure!(
            success,
            "GitHub CLI has no usable signed-in account. Complete its browser sign-in, then connect again."
        );
        let token = String::from_utf8(token)
            .map_err(|_| anyhow!("GitHub CLI returned invalid authorization"))?
            .trim()
            .to_owned();
        ensure!(
            !token.is_empty() && !token.contains(char::is_whitespace),
            "GitHub CLI returned invalid authorization"
        );
        let mut client = Client(Self {
            credential: Credential {
                login: String::new(),
                token,
            },
        });
        let user = client.account(control)?;
        ensure!(
            !user.login.is_empty() && user.login.len() <= 100,
            "GitHub returned an invalid account"
        );
        client.0.credential.login = user.login.clone();
        SecureStore.save(&client.0.credential)?;
        Ok(user.login)
    }
}
impl Transport for GhTransport {
    fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
        ensure!(
            request.endpoint.starts_with("repos/")
                || request.endpoint == "user"
                || request.endpoint == "graphql",
            "Unsupported GitHub API destination"
        );
        ensure!(
            !request.endpoint.contains(['\n', '\r', '#']) && !request.endpoint.starts_with('/'),
            "Invalid GitHub API destination"
        );
        let mut command = gh_command()?;
        // API routing and credential lookup must not inherit CLI api_host or
        // http_unix_socket overrides. The explicit `auth token` handoff above
        // still uses the user's CLI configuration to identify their account.
        let _configuration = isolate_api_configuration(&mut command)?;
        command.env("GH_TOKEN", &self.credential.token).args([
            "api",
            "--hostname",
            "github.com",
            "--include",
            "--method",
            request.method,
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2026-03-10",
        ]);
        let input = request
            .body
            .map(|body| serde_json::to_vec(&body))
            .transpose()?;
        if input.is_some() {
            command.args(["--input", "-"]);
        }
        command.arg(format!("https://api.github.com/{}", request.endpoint));
        let (_, bytes) = run(
            command,
            input.as_deref(),
            control,
            MAX_RESPONSE + 64 * 1024,
            Duration::from_secs(90),
        )?;
        parse_response(&bytes)
    }
}
fn isolate_api_configuration(command: &mut Command) -> Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("gitturtle-github-api-")
        .tempdir()
        .map_err(|_| anyhow!("Could not prepare isolated GitHub API configuration"))?;
    command.env("GH_CONFIG_DIR", directory.path());
    Ok(directory)
}
fn gh_command() -> Result<Command> {
    let executable=["/opt/homebrew/bin/gh","/usr/local/bin/gh","/usr/bin/gh"].into_iter().map(std::path::Path::new).find(|path|path.is_file()).ok_or_else(||anyhow!("Install GitHub CLI from cli.github.com, sign in with its browser flow, then connect the account."))?;
    let mut command = Command::new(executable);
    command
        .current_dir(std::env::temp_dir())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env("GH_PAGER", "cat")
        .env("GH_TELEMETRY_DISABLED", "1");
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "GH_HOST",
        "GH_REPO",
        "GH_FORCE_TTY",
    ] {
        command.env_remove(key);
    }
    Ok(command)
}
fn parse_response(bytes: &[u8]) -> Result<Response> {
    let (position, separator) = bytes
        .windows(4)
        .position(|b| b == b"\r\n\r\n")
        .map(|p| (p, 4))
        .or_else(|| bytes.windows(2).position(|b| b == b"\n\n").map(|p| (p, 2)))
        .ok_or_else(|| {
            anyhow!("GitHub response was incomplete; its remote outcome could not be established")
        })?;
    ensure!(
        position <= 64 * 1024,
        "GitHub response headers exceeded the limit"
    );
    let header = std::str::from_utf8(&bytes[..position])
        .map_err(|_| anyhow!("GitHub response headers are invalid"))?;
    let mut lines = header.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| anyhow!("GitHub response status is invalid"))?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    Ok(Response {
        status,
        headers,
        body: bytes[position + separator..].to_vec(),
    })
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<()> {
    let descriptor = pipe.as_raw_fd();
    // SAFETY: descriptor belongs to the live pipe, and fcntl does not retain it.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    ensure!(
        flags >= 0
            && unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "Could not configure bounded GitHub subprocess I/O"
    );
    Ok(())
}
#[cfg(unix)]
fn run(
    mut command: Command,
    input: Option<&[u8]>,
    control: &OperationControl,
    limit: usize,
    deadline: Duration,
) -> Result<(bool, Vec<u8>)> {
    use std::os::unix::process::CommandExt;
    ensure!(
        !control.is_cancelled(),
        "GitHub operation cancelled before dispatch"
    );
    command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| anyhow!("Could not start GitHub CLI"))?;
    let result = (|| {
        let mut stdin = Some(
            child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("GitHub input pipe unavailable"))?,
        );
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("GitHub output pipe unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("GitHub error pipe unavailable"))?;
        nonblocking(stdin.as_ref().expect("input pipe"))?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let input = input.unwrap_or_default();
        let mut sent = 0;
        let mut output = Vec::new();
        let mut stderr_bytes = 0;
        let mut stdout_done = false;
        let mut stderr_done = false;
        let start = Instant::now();
        let mut status = None;
        loop {
            ensure!(!control.is_cancelled(), "GitHub operation cancelled");
            ensure!(
                start.elapsed() < deadline,
                "GitHub operation exceeded its deadline"
            );
            if let Some(pipe) = stdin.as_mut() {
                if sent < input.len() {
                    match pipe.write(&input[sent..]) {
                        Ok(count) => sent += count,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => bail!("GitHub request input was interrupted"),
                    }
                }
                if sent == input.len() {
                    stdin = None;
                }
            }
            let mut buffer = [0u8; 16384];
            if !stdout_done {
                match stdout.read(&mut buffer) {
                    Ok(0) => stdout_done = true,
                    Ok(count) => {
                        ensure!(
                            output.len() + count <= limit,
                            "GitHub response exceeds the bounded output limit"
                        );
                        output.extend_from_slice(&buffer[..count]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => bail!("GitHub response stream was interrupted"),
                }
            }
            if !stderr_done {
                match stderr.read(&mut buffer) {
                    Ok(0) => stderr_done = true,
                    Ok(count) => {
                        stderr_bytes += count;
                        ensure!(
                            stderr_bytes <= 64 * 1024,
                            "GitHub diagnostic output exceeded its limit"
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => bail!("GitHub diagnostic stream was interrupted"),
                }
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let Some(status) = status
                && stdout_done
                && stderr_done
            {
                return Ok((status.success(), output));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        // SAFETY: the child was started in its own process group. Negative PID
        // targets that group, so pipe-holding descendants are stopped as well.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
#[cfg(not(unix))]
fn run(
    _: Command,
    _: Option<&[u8]>,
    _: &OperationControl,
    _: usize,
    _: Duration,
) -> Result<(bool, Vec<u8>)> {
    bail!("GitHub CLI transport is not yet available on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_and_rate_limit_are_parsed_without_diagnostics() {
        let response =
            parse_response(b"HTTP/2.0 429 Too Many Requests\r\nRetry-After: 30\r\n\r\n{}").unwrap();
        assert_eq!(response.status, 429);
        assert_eq!(response.headers["retry-after"], "30");
        assert_eq!(response.body, b"{}");
    }
    #[test]
    fn token_debug_never_displays_secret() {
        let credential = Credential {
            login: "fixture".into(),
            token: "test-secret".into(),
        };
        assert!(!format!("{credential:?}").contains("test-secret"));
    }
    #[cfg(unix)]
    #[test]
    fn body_pipe_reaches_eof_and_output_is_bounded() {
        let (success, bytes) = run(
            Command::new("/bin/cat"),
            Some(b"fixture request body"),
            &OperationControl::default(),
            1024,
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(success);
        assert_eq!(bytes, b"fixture request body");
        assert!(
            run(
                Command::new("/bin/cat"),
                Some(&[b'x'; 4096]),
                &OperationControl::default(),
                1024,
                Duration::from_secs(2)
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_stops_subprocess_and_does_not_replay() {
        let control = OperationControl::default();
        let cancelling = control.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            cancelling.cancel();
        });
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let start = Instant::now();
        assert!(
            run(command, None, &control, 1024, Duration::from_secs(3))
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        handle.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn deadlines_include_descendants_holding_stdout_after_the_parent_exits() {
        let fixture = tempfile::tempdir().unwrap();
        let marker = fixture.path().join("started");
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf started > \"$1\"; sleep 30 &", "fixture"]);
        command.arg(&marker);
        let start = Instant::now();
        let error = run(
            command,
            None,
            &OperationControl::default(),
            1024,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert_eq!(std::fs::read(marker).unwrap(), b"started");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires installed gh; dummy token, Unix socket and loopback proxy only"]
    fn installed_cli_api_isolation_blocks_configured_token_rerouting() {
        use std::{net::TcpListener, os::unix::net::UnixListener};

        fn read_headers(stream: &mut impl Read) -> String {
            let mut bytes = Vec::new();
            let mut byte = [0];
            while !bytes.ends_with(b"\r\n\r\n") {
                assert!(bytes.len() < 16384);
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                bytes.push(byte[0]);
            }
            String::from_utf8(bytes).unwrap()
        }
        fn await_connection<T>(mut accept: impl FnMut() -> std::io::Result<T>) -> T {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match accept() {
                    Ok(connection) => return connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "fixture connection deadline");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
        }

        let fixture = tempfile::Builder::new()
            .prefix("gh-origin-")
            .tempdir_in("/tmp")
            .unwrap();
        let configuration = fixture.path().join("cli-config");
        std::fs::create_dir(&configuration).unwrap();
        let socket_path = fixture.path().join("api.sock");
        let config_bytes = format!("version: 1\nhttp_unix_socket: {}\n", socket_path.display());
        std::fs::write(configuration.join("config.yml"), &config_bytes).unwrap();
        std::fs::write(
            configuration.join("hosts.yml"),
            "github.com:\n    user: fixture\n    api_host: override.invalid\n",
        )
        .unwrap();
        let socket = UnixListener::bind(&socket_path).unwrap();
        socket.set_nonblocking(true).unwrap();
        let accepting = socket.try_clone().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = await_connection(|| accepting.accept());
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let request = read_headers(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap();
            request
        });
        let mut inherited = gh_command().unwrap();
        inherited
            .env("GH_CONFIG_DIR", &configuration)
            .env("GH_TOKEN", "fixture-dummy-token");
        inherited.args([
            "api",
            "--hostname",
            "github.com",
            "--include",
            "--method",
            "GET",
            "user",
        ]);
        assert!(
            run(
                inherited,
                None,
                &OperationControl::default(),
                16384,
                Duration::from_secs(3)
            )
            .unwrap()
            .0
        );
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.contains("host: override.invalid\r\n"));
        assert!(request.contains("authorization: token fixture-dummy-token\r\n"));

        // The isolated command must use normal HTTPS. A local refusing proxy
        // prevents DNS/TLS/provider traffic and observes only its CONNECT line.
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = await_connection(|| proxy.accept());
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let request = read_headers(&mut stream);
            stream.write_all(b"HTTP/1.1 502 Fixture Refusal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            request
        });
        let mut isolated = gh_command().unwrap();
        isolated
            .env("GH_CONFIG_DIR", &configuration)
            .env("GH_TOKEN", "fixture-dummy-token");
        for key in [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            isolated.env(key, &proxy_url);
        }
        isolated.env("NO_PROXY", "").env("no_proxy", "");
        let private = isolate_api_configuration(&mut isolated).unwrap();
        isolated.args([
            "api",
            "--hostname",
            "github.com",
            "--include",
            "--method",
            "GET",
            "https://api.github.com/user",
        ]);
        let (success, _) = run(
            isolated,
            None,
            &OperationControl::default(),
            16384,
            Duration::from_secs(3),
        )
        .unwrap();
        assert!(!success);
        let request = server.join().unwrap();
        assert!(request.starts_with("CONNECT api.github.com:443 HTTP/1.1\r\n"));
        assert!(!request.contains("fixture-dummy-token"));
        assert_eq!(
            socket.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(
            std::fs::read(configuration.join("config.yml")).unwrap(),
            config_bytes.as_bytes()
        );
        for entry in std::fs::read_dir(private.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(!String::from_utf8_lossy(&bytes).contains("fixture-dummy-token"));
            }
        }
        let private_path = private.path().to_owned();
        drop(private);
        assert!(!private_path.exists());
    }
}
