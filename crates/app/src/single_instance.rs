//! Single-instance Linux IPC over a private user-runtime Unix domain socket.
//!
//! Launching `gitturtle [path...]` (from a terminal, file manager, or Walker)
//! while an instance using the same configuration directory is already running
//! forwards the byte-safe target paths and optional `XDG_ACTIVATION_TOKEN` to
//! the running instance so it opens new tabs and raises its window instead of
//! starting a second process that contends on preference/session JSON stores.

use futures::channel::mpsc;
use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use rustix::fd::OwnedFd;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const MAGIC: &[u8; 4] = b"GTSI";
const PROTOCOL_VERSION: u8 = 1;
const REPLY_OK: &[u8; 3] = b"OK\n";
const REPLY_REJECT: &[u8; 3] = b"NO\n";

const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_CONFIG_TAG_BYTES: usize = 4096;
const MAX_TOKEN_BYTES: usize = 512;
const MAX_PATHS: usize = 32;
const MAX_PATH_BYTES: usize = 4096;

const CLIENT_IO_TIMEOUT: Duration = Duration::from_millis(250);
const SERVER_IO_TIMEOUT: Duration = Duration::from_millis(250);

static TEMP_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

/// An activation request forwarded from a secondary launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Activation {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) activation_token: Option<String>,
}

/// Result of probing for an existing instance during CLI launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandOff {
    /// Forwarded to a running instance; caller should exit cleanly.
    HandedOff,
    /// No matching instance answered; caller should start normally.
    Proceed,
}

/// Active socket listener guard; wakes the `poll(2)` accept thread via an owned
/// `eventfd` (never reconnecting through the filesystem path) and unlinks the
/// socket on shutdown only if `(dev, ino)` still match this process's socket.
pub(crate) struct ListenerGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
    stop: Arc<AtomicBool>,
    wake_fd: Arc<OwnedFd>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ListenerGuard {
    pub(crate) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = rustix::io::write(&*self.wake_fd, &1u64.to_ne_bytes());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        unlink_if_same_inode(&self.path, self.dev, self.ino);
    }
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    socket_path: PathBuf,
    config_tag: Vec<u8>,
}

/// Returns whether single-instance IPC is explicitly disabled by environment.
fn disabled_by_env() -> bool {
    std::env::var_os("GITTURTLE_NO_SINGLE_INSTANCE")
        .is_some_and(|v| !v.is_empty() && v != OsStr::new("0"))
}

/// Resolves the private Unix socket path and config-dir identity tag for the
/// current process environment.
///
/// Fails closed (returns `None`) if `$XDG_RUNTIME_DIR` is missing, relative,
/// not a directory, not owned by the current effective UID, or group/world
/// accessible.
fn current_endpoint() -> Option<Endpoint> {
    if disabled_by_env() {
        return None;
    }
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg_config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    endpoint_for(
        &runtime_dir,
        xdg_config.as_deref(),
        home.as_deref(),
        rustix::process::geteuid().as_raw(),
    )
}

fn endpoint_for(
    runtime_dir: &Path,
    xdg_config: Option<&Path>,
    home: Option<&Path>,
    expected_uid: u32,
) -> Option<Endpoint> {
    if !runtime_dir.is_absolute() {
        return None;
    }
    let meta = fs::symlink_metadata(runtime_dir).ok()?;
    if !meta.file_type().is_dir() || meta.uid() != expected_uid || (meta.mode() & 0o077) != 0 {
        return None;
    }

    fn absolute(path: Option<&Path>) -> Option<&Path> {
        path.filter(|p| p.is_absolute())
    }
    let default_config = absolute(home).map(|h| h.join(".config"));
    let active_config = absolute(xdg_config)
        .map(Path::to_path_buf)
        .or_else(|| default_config.clone())?;
    let active_dir = active_config.join("gitturtle");
    let config_tag = canonical_or_raw(&active_dir).into_os_string().into_vec();
    if config_tag.is_empty() || config_tag.len() > MAX_CONFIG_TAG_BYTES {
        return None;
    }

    let uses_default = default_config
        .as_deref()
        .is_some_and(|default| default == active_config);
    let socket_name = if uses_default {
        "gitturtle.sock".to_owned()
    } else {
        format!(
            "gitturtle-{:012x}.sock",
            fnv1a_64(&config_tag) & 0xffff_ffff_ffff
        )
    };
    Some(Endpoint {
        socket_path: runtime_dir.join(socket_name),
        config_tag,
    })
}

fn canonical_or_raw(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn resolve_cli_paths(args: impl IntoIterator<Item = OsString>, cwd: Option<&Path>) -> Vec<PathBuf> {
    args.into_iter()
        .take(MAX_PATHS)
        .filter(|arg| !arg.is_empty())
        .map(|arg| {
            let path = PathBuf::from(arg);
            if path.is_absolute() {
                path
            } else if let Some(cwd) = cwd {
                cwd.join(path)
            } else {
                path
            }
        })
        .collect()
}

fn current_activation_token() -> Option<String> {
    std::env::var("XDG_ACTIVATION_TOKEN").ok().filter(|token| {
        !token.is_empty() && token.len() <= MAX_TOKEN_BYTES && !token.chars().any(char::is_control)
    })
}

/// Probes an existing instance for the current config directory and forwards
/// CLI repository paths plus `XDG_ACTIVATION_TOKEN`.
pub(crate) fn try_hand_off_cli_launch() -> HandOff {
    let Some(endpoint) = current_endpoint() else {
        return HandOff::Proceed;
    };
    let cwd = std::env::current_dir().ok();
    let paths = resolve_cli_paths(std::env::args_os().skip(1), cwd.as_deref());
    let activation = Activation {
        paths,
        activation_token: current_activation_token(),
    };
    try_send_to(&endpoint, &activation)
}

fn try_send_to(endpoint: &Endpoint, activation: &Activation) -> HandOff {
    let Ok(mut stream) = UnixStream::connect(&endpoint.socket_path) else {
        return HandOff::Proceed;
    };
    if !peer_matches_current_user(&stream) {
        return HandOff::Proceed;
    }
    let _ = stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT));
    let Some(frame) = encode_frame(&endpoint.config_tag, activation) else {
        return HandOff::Proceed;
    };
    if stream.write_all(&frame).is_err() {
        return HandOff::Proceed;
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut reply = [0u8; 3];
    if stream.read_exact(&mut reply).is_ok() && &reply == REPLY_OK {
        HandOff::HandedOff
    } else {
        HandOff::Proceed
    }
}

/// Starts listening on the single-instance Unix socket for the current config
/// directory, forwarding verified same-UID activation requests over `tx`.
pub(crate) fn listen(tx: mpsc::Sender<Activation>) -> Option<ListenerGuard> {
    let endpoint = current_endpoint()?;
    listen_at(endpoint, tx)
}

fn listen_at(endpoint: Endpoint, mut tx: mpsc::Sender<Activation>) -> Option<ListenerGuard> {
    let (listener, dev, ino) = bind_private_socket(&endpoint.socket_path).ok()?;
    if listener.set_nonblocking(true).is_err() {
        drop(listener);
        unlink_if_same_inode(&endpoint.socket_path, dev, ino);
        return None;
    }
    let Ok(wake_fd) = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK) else {
        drop(listener);
        unlink_if_same_inode(&endpoint.socket_path, dev, ino);
        return None;
    };
    let wake_fd = Arc::new(wake_fd);
    let thread_wake = Arc::clone(&wake_fd);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let config_tag = endpoint.config_tag;
    let Ok(handle) = std::thread::Builder::new()
        .name("gitturtle-ipc".into())
        .spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let mut fds = [
                    PollFd::new(&listener, PollFlags::IN),
                    PollFd::new(&*thread_wake, PollFlags::IN),
                ];
                match poll(&mut fds, None) {
                    Ok(_) => {}
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(_) => break,
                }
                if thread_stop.load(Ordering::Acquire) || fds[1].revents().contains(PollFlags::IN) {
                    break;
                }
                loop {
                    if thread_stop.load(Ordering::Acquire) {
                        break;
                    }
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            handle_client(stream, &config_tag, &mut tx);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => return,
                    }
                }
            }
        })
    else {
        unlink_if_same_inode(&endpoint.socket_path, dev, ino);
        return None;
    };
    Some(ListenerGuard {
        path: endpoint.socket_path,
        dev,
        ino,
        stop,
        wake_fd,
        handle: Some(handle),
    })
}

fn bind_private_socket(socket_path: &Path) -> io::Result<(UnixListener, u64, u64)> {
    let parent = socket_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket path has no parent"))?;
    let file_name = socket_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gitturtle.sock");

    for attempt in 0..2 {
        // Bind an exclusive temporary sibling socket inside the verified 0700
        // runtime directory, lock down its permissions to 0600, capture its
        // `(dev, ino)` before publishing, then publish with `hard_link` so an
        // existing live socket or non-socket entry is never overwritten.
        let seq = TEMP_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(".{file_name}.{}.{seq}.tmp", std::process::id()));
        let listener = UnixListener::bind(&temp_path)?;
        if let Err(error) = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600)) {
            drop(listener);
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        let meta = match fs::symlink_metadata(&temp_path) {
            Ok(meta) if meta.file_type().is_socket() => meta,
            Ok(_) => {
                drop(listener);
                let _ = fs::remove_file(&temp_path);
                return Err(io::Error::other("bound temporary path is not a socket"));
            }
            Err(error) => {
                drop(listener);
                let _ = fs::remove_file(&temp_path);
                return Err(error);
            }
        };
        let (dev, ino) = (meta.dev(), meta.ino());
        let publish = fs::hard_link(&temp_path, socket_path);
        let _ = fs::remove_file(&temp_path);
        match publish {
            Ok(()) => return Ok((listener, dev, ino)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt == 0 => {
                drop(listener);
                reclaim_stale_socket(socket_path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                drop(listener);
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another instance is already listening",
                ));
            }
            Err(error) => {
                drop(listener);
                return Err(error);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "another instance is already listening",
    ))
}

fn reclaim_stale_socket(socket_path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(socket_path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !meta.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to unlink non-socket path",
        ));
    }
    match UnixStream::connect(socket_path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "another instance is already listening",
        )),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            unlink_if_same_inode(socket_path, meta.dev(), meta.ino());
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn handle_client(
    mut stream: UnixStream,
    expected_config_tag: &[u8],
    tx: &mut mpsc::Sender<Activation>,
) {
    if !peer_matches_current_user(&stream) {
        let _ = stream.write_all(REPLY_REJECT);
        return;
    }
    let _ = stream.set_write_timeout(Some(SERVER_IO_TIMEOUT));
    let deadline = Instant::now() + SERVER_IO_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            let _ = stream.write_all(REPLY_REJECT);
            return;
        };
        if remaining.is_zero() {
            let _ = stream.write_all(REPLY_REJECT);
            return;
        }
        let _ = stream.set_read_timeout(Some(remaining));
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_FRAME_BYTES {
                    let _ = stream.write_all(REPLY_REJECT);
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                let _ = stream.write_all(REPLY_REJECT);
                return;
            }
        }
    }
    match decode_frame(&buf) {
        Some((config_tag, activation)) if config_tag == expected_config_tag => {
            if tx.try_send(activation).is_ok() {
                let _ = stream.write_all(REPLY_OK);
            } else {
                let _ = stream.write_all(REPLY_REJECT);
            }
        }
        _ => {
            let _ = stream.write_all(REPLY_REJECT);
        }
    }
}

fn peer_matches_current_user(stream: &UnixStream) -> bool {
    let Ok(cred) = rustix::net::sockopt::socket_peercred(stream) else {
        return false;
    };
    cred.uid == rustix::process::geteuid()
}

fn unlink_if_same_inode(path: &Path, dev: u64, ino: u64) {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_socket()
        && meta.dev() == dev
        && meta.ino() == ino
    {
        let _ = fs::remove_file(path);
    }
}

fn encode_frame(config_tag: &[u8], activation: &Activation) -> Option<Vec<u8>> {
    if config_tag.len() > MAX_CONFIG_TAG_BYTES || activation.paths.len() > MAX_PATHS {
        return None;
    }
    let token_bytes = activation
        .activation_token
        .as_deref()
        .filter(|t| t.len() <= MAX_TOKEN_BYTES && !t.chars().any(char::is_control))
        .map(str::as_bytes)
        .unwrap_or(&[]);

    let mut out = Vec::with_capacity(64 + config_tag.len() + token_bytes.len());
    out.extend_from_slice(MAGIC);
    out.push(PROTOCOL_VERSION);
    out.push(0); // reserved flags
    out.extend_from_slice(&u16::try_from(config_tag.len()).ok()?.to_le_bytes());
    out.extend_from_slice(config_tag);
    out.extend_from_slice(&u16::try_from(token_bytes.len()).ok()?.to_le_bytes());
    out.extend_from_slice(token_bytes);
    out.extend_from_slice(&u16::try_from(activation.paths.len()).ok()?.to_le_bytes());
    for path in &activation.paths {
        let bytes = path.as_os_str().as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) {
            return None;
        }
        out.extend_from_slice(&u32::try_from(bytes.len()).ok()?.to_le_bytes());
        out.extend_from_slice(bytes);
    }
    (out.len() <= MAX_FRAME_BYTES).then_some(out)
}

fn decode_frame(input: &[u8]) -> Option<(Vec<u8>, Activation)> {
    if input.len() > MAX_FRAME_BYTES {
        return None;
    }
    let mut cursor = input;
    fn take<'a>(cursor: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        if cursor.len() < n {
            return None;
        }
        let (head, tail) = cursor.split_at(n);
        *cursor = tail;
        Some(head)
    }

    if take(&mut cursor, 4)? != MAGIC {
        return None;
    }
    let version = take(&mut cursor, 1)?[0];
    if version != PROTOCOL_VERSION {
        return None;
    }
    let flags = take(&mut cursor, 1)?[0];
    if flags != 0 {
        return None;
    }

    let tag_len = usize::from(u16::from_le_bytes(take(&mut cursor, 2)?.try_into().ok()?));
    if tag_len == 0 || tag_len > MAX_CONFIG_TAG_BYTES {
        return None;
    }
    let config_tag = take(&mut cursor, tag_len)?.to_vec();

    let token_len = usize::from(u16::from_le_bytes(take(&mut cursor, 2)?.try_into().ok()?));
    if token_len > MAX_TOKEN_BYTES {
        return None;
    }
    let token_bytes = take(&mut cursor, token_len)?;
    let activation_token = if token_bytes.is_empty() {
        None
    } else {
        let token = std::str::from_utf8(token_bytes).ok()?;
        if token.chars().any(char::is_control) {
            return None;
        }
        Some(token.to_owned())
    };

    let path_count = usize::from(u16::from_le_bytes(take(&mut cursor, 2)?.try_into().ok()?));
    if path_count > MAX_PATHS {
        return None;
    }
    let mut paths = Vec::with_capacity(path_count);
    for _ in 0..path_count {
        let len =
            usize::try_from(u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?)).ok()?;
        if len == 0 || len > MAX_PATH_BYTES {
            return None;
        }
        let raw = take(&mut cursor, len)?;
        if raw.contains(&0) {
            return None;
        }
        paths.push(PathBuf::from(OsString::from_vec(raw.to_vec())));
    }
    if !cursor.is_empty() {
        return None;
    }
    Some((
        config_tag,
        Activation {
            paths,
            activation_token,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_runtime_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn endpoint_uses_default_socket_for_default_config_and_scoped_for_custom() {
        let runtime = private_runtime_dir();
        let home = runtime.path().join("home");
        let custom_cfg = runtime.path().join("custom-cfg");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&custom_cfg).unwrap();
        let uid = rustix::process::geteuid().as_raw();

        let default_ep = endpoint_for(runtime.path(), None, Some(&home), uid).unwrap();
        assert_eq!(
            default_ep.socket_path,
            runtime.path().join("gitturtle.sock")
        );

        let scoped_ep = endpoint_for(runtime.path(), Some(&custom_cfg), Some(&home), uid).unwrap();
        assert_ne!(scoped_ep.socket_path, default_ep.socket_path);
        assert!(
            scoped_ep
                .socket_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("gitturtle-")
        );
        assert_ne!(scoped_ep.config_tag, default_ep.config_tag);
    }

    #[test]
    fn endpoint_refuses_group_or_world_accessible_runtime_dir() {
        let runtime = tempfile::tempdir().unwrap();
        fs::set_permissions(runtime.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let uid = rustix::process::geteuid().as_raw();
        assert!(endpoint_for(runtime.path(), None, Some(Path::new("/home/u")), uid).is_none());
    }

    #[test]
    fn round_trip_preserves_non_utf8_paths_activation_token_and_rejects_mismatched_config() {
        let runtime = private_runtime_dir();
        let endpoint = Endpoint {
            socket_path: runtime.path().join("gitturtle.sock"),
            config_tag: b"/home/u/.config/gitturtle".to_vec(),
        };
        let (tx, mut rx) = mpsc::channel(16);
        let mut guard = listen_at(endpoint.clone(), tx).expect("bind socket");

        let meta = fs::symlink_metadata(&endpoint.socket_path).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);

        let raw_path = PathBuf::from(OsString::from_vec(b"/tmp/r\xffpo".to_vec()));
        let sent = Activation {
            paths: vec![raw_path],
            activation_token: Some("xdg-token-123".into()),
        };
        assert_eq!(try_send_to(&endpoint, &sent), HandOff::HandedOff);
        let received = rx.try_recv().expect("activation received");
        assert_eq!(received, sent);

        // A client for a different config directory must be rejected.
        let other = Endpoint {
            socket_path: endpoint.socket_path.clone(),
            config_tag: b"/tmp/isolated-qa/gitturtle".to_vec(),
        };
        assert_eq!(try_send_to(&other, &sent), HandOff::Proceed);
        assert!(rx.try_recv().is_err());

        guard.shutdown();
        assert!(!endpoint.socket_path.exists());
    }

    #[test]
    fn shutdown_returns_promptly_even_when_socket_is_unlinked_externally() {
        let runtime = private_runtime_dir();
        let endpoint = Endpoint {
            socket_path: runtime.path().join("gitturtle.sock"),
            config_tag: b"/cfg".to_vec(),
        };
        let (tx, _rx) = mpsc::channel(4);
        let mut guard = listen_at(endpoint.clone(), tx).expect("bind socket");
        fs::remove_file(&endpoint.socket_path).unwrap();
        guard.shutdown();
    }

    #[test]
    fn bind_recovers_stale_socket_never_clobbers_live_socket_and_refuses_regular_file() {
        let runtime = private_runtime_dir();
        let stale_path = runtime.path().join("gitturtle.sock");
        // Create and immediately drop a socket to leave a stale socket file.
        {
            let _stale = UnixListener::bind(&stale_path).unwrap();
        }
        assert!(stale_path.exists());
        let (tx, _rx) = mpsc::channel(4);
        let endpoint = Endpoint {
            socket_path: stale_path.clone(),
            config_tag: b"/cfg".to_vec(),
        };
        let guard = listen_at(endpoint, tx).expect("recovers stale socket");
        // A second bind against a live listener must refuse with AddrInUse.
        assert_eq!(
            bind_private_socket(&stale_path).unwrap_err().kind(),
            io::ErrorKind::AddrInUse
        );
        drop(guard);
        assert!(!stale_path.exists());

        // Refuses to unlink a regular file placed at the socket path.
        fs::write(&stale_path, b"not a socket").unwrap();
        assert!(bind_private_socket(&stale_path).is_err());
        assert_eq!(fs::read(&stale_path).unwrap(), b"not a socket");
    }

    #[test]
    fn resolve_cli_paths_expands_relative_against_cwd() {
        let resolved = resolve_cli_paths(
            [OsString::from("repo"), OsString::from("/abs/repo")],
            Some(Path::new("/work/dir")),
        );
        assert_eq!(
            resolved,
            vec![PathBuf::from("/work/dir/repo"), PathBuf::from("/abs/repo")]
        );
    }

    #[test]
    fn decode_frame_rejects_nonzero_flags_trailing_bytes_nul_and_oversize() {
        let act = Activation {
            paths: vec![PathBuf::from("/tmp/ok")],
            activation_token: None,
        };
        let mut frame = encode_frame(b"/cfg", &act).unwrap();
        assert!(decode_frame(&frame).is_some());
        frame[5] = 1; // non-zero reserved flags
        assert!(decode_frame(&frame).is_none());
        frame[5] = 0;
        frame.push(0);
        assert!(decode_frame(&frame).is_none());

        let bad_path = Activation {
            paths: vec![PathBuf::from(OsString::from_vec(b"/tmp/a\0b".to_vec()))],
            activation_token: None,
        };
        assert!(encode_frame(b"/cfg", &bad_path).is_none());
    }
}
