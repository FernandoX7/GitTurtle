//! Hands a private copy to the desktop's default application.
//!
//! The XDG Desktop Portal's `OpenURI.OpenFile` comes first: it receives the
//! read-only descriptor rather than a path, works in a sandbox, and lets the
//! portal choose (or ask for) the application by the copy's type as GIO
//! determines it. GIO reads the suffix [`passive`](super::passive) wrote only
//! when one type alone claims it and otherwise sniffs the content, so before
//! every portal request `gio` itself is asked, and the copy goes on only when
//! its answer is one of the kind's media types. Only when no portal is there
//! to answer does `xdg-open` run, from a fixed system path with a fixed
//! argument array and no shell, and only after `xdg-mime`, which `xdg-open`
//! itself asks, types the copy as the kind it is and names a default
//! application for that type. A portal that answered, refused or was
//! cancelled is never followed by a second attempt, so nothing opens twice or
//! after the user said no.
use super::{passive::Kind, store::Copy};
use crate::rich_preview::ExternalPreviewSlot;
use ashpd::{
    PortalError,
    desktop::{ResponseError, open_uri::OpenFileRequest},
    zbus,
};
use futures::{FutureExt as _, channel::oneshot};
use gpui_kit::BackgroundExecutor;
use std::{
    ffi::OsString,
    io::Read as _,
    os::{fd::AsFd as _, unix::process::CommandExt as _},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// Where `xdg-open` is looked for, in the order a default `PATH` searches.
/// `PATH` itself is not consulted.
pub(super) const XDG_OPEN: [&str; 4] = [
    "/usr/local/bin/xdg-open",
    "/usr/bin/xdg-open",
    "/bin/xdg-open",
    "/run/current-system/sw/bin/xdg-open",
];
/// Where GLib's `gio` is looked for; it types a file as the portal will.
pub(super) const GIO: [&str; 4] = [
    "/usr/local/bin/gio",
    "/usr/bin/gio",
    "/bin/gio",
    "/run/current-system/sw/bin/gio",
];
/// Where `xdg-mime` is looked for, beside `xdg-open`, which runs it to type
/// the file it opens.
pub(super) const XDG_MIME: [&str; 4] = [
    "/usr/local/bin/xdg-mime",
    "/usr/bin/xdg-mime",
    "/bin/xdg-mime",
    "/run/current-system/sw/bin/xdg-mime",
];
/// `gio` and `xdg-mime` sniff one small file or read one list; past this an
/// answer does not count and nothing opens.
const QUERY_WAIT: Duration = Duration::from_secs(5);
/// The most of an answer that is read. `gio info` prints the copy's path
/// twice before the attribute; a longer answer is cut and so refused.
const MAX_ANSWER: u64 = 16 * 1024;
/// An application chooser can wait for the user; past this the portal is
/// reported as not having answered, and nothing else is tried.
const PORTAL_WAIT: Duration = Duration::from_secs(120);
/// `xdg-open` runs the application in the foreground, so a quick exit is a
/// failure to report and a longer run is the application.
const LAUNCHER_FAILURE_WAIT: Duration = Duration::from_secs(2);

/// What the portal's answer allows next.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Portal {
    Opened,
    /// The user dismissed the portal's application chooser.
    Cancelled,
    /// The portal answered with an error, such as no application.
    Refused(String),
    /// The request may still be in progress or have opened something.
    NoAnswer(String),
    /// No portal service or OpenURI interface exists, so nothing was asked.
    Unavailable,
}

pub(super) async fn open(
    copy: Copy,
    kind: Kind,
    slot: ExternalPreviewSlot,
    executor: BackgroundExecutor,
) -> Result<(), String> {
    let environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let Some(gio) = find_launcher(&GIO.map(Path::new)) else {
        return Err("This needs gio (GLib) to check what the desktop would open the copy as, and it is not installed.".into());
    };
    let (path, directory, query_environment) = (
        copy.path.clone(),
        copy.directory.clone(),
        environment.clone(),
    );
    off_thread(
        &executor,
        QUERY_WAIT + Duration::from_secs(1),
        "gio",
        move || gio_agrees(kind, &gio, &path, &directory, query_environment, QUERY_WAIT),
    )
    .await?;
    let request = portal(&copy).fuse();
    let deadline = executor.timer(PORTAL_WAIT).fuse();
    futures::pin_mut!(request, deadline);
    let answer = futures::select! {
        answer = request => answer,
        () = deadline => Portal::NoAnswer("no answer within two minutes".into()),
    };
    match answer {
        Portal::Opened => Ok(()),
        Portal::Cancelled => {
            Err("The application chooser was closed, so nothing was opened.".into())
        }
        Portal::Refused(error) => Err(format!("The desktop portal refused it: {error}.")),
        Portal::NoAnswer(error) => Err(format!(
            "The desktop portal did not finish ({error}); it may still open the copy."
        )),
        Portal::Unavailable => {
            if !kind.typed_by_signature() {
                return Err("Text opens only through the desktop portal, which is not running here; xdg-open would choose an application from what the text contains, not its type.".into());
            }
            let (Some(xdg_mime), Some(program)) = (
                find_launcher(&XDG_MIME.map(Path::new)),
                find_launcher(&XDG_OPEN.map(Path::new)),
            ) else {
                return Err("This needs the desktop portal (xdg-desktop-portal), or xdg-open with xdg-mime, and neither is installed.".into());
            };
            let (path, directory, query_environment) = (
                copy.path.clone(),
                copy.directory.clone(),
                environment.clone(),
            );
            // Two queries, each bounded by `QUERY_WAIT`.
            let wait = QUERY_WAIT * 2 + Duration::from_secs(1);
            off_thread(&executor, wait, "xdg-mime", move || {
                xdg_open_agrees(
                    kind,
                    &xdg_mime,
                    &path,
                    &directory,
                    query_environment,
                    QUERY_WAIT,
                )
            })
            .await?;
            let mut child = launcher_command(&program, &copy.path, &copy.directory, environment)
                .spawn()
                .map_err(|error| format!("xdg-open did not start: {error}."))?;
            let (sender, exited) = oneshot::channel();
            // Reaped here; the slot bounds how many such launchers run at once.
            std::thread::spawn(move || {
                let _ = sender.send(child.wait());
                drop(slot);
            });
            let exited = exited.fuse();
            let running = executor.timer(LAUNCHER_FAILURE_WAIT).fuse();
            futures::pin_mut!(exited, running);
            futures::select! {
                status = exited => match status {
                    Ok(Ok(status)) => launcher_result(status),
                    Ok(Err(error)) => Err(format!(
                        "xdg-open could not be waited for ({error}); it may still open the copy."
                    )),
                    Err(_) => Err("xdg-open's result was lost; it may still open the copy.".into()),
                },
                () = running => Ok(()),
            }
        }
    }
}

async fn portal(copy: &Copy) -> Portal {
    let connection = match zbus::connection::Builder::session() {
        Ok(builder) => builder.build().await,
        Err(error) => Err(error),
    };
    let Ok(connection) = connection else {
        return Portal::Unavailable;
    };
    // No write access is requested (the portal's default, which the request
    // leaves unset), and the descriptor is read-only, which the portal cannot
    // turn into write access for the application.
    let request = OpenFileRequest::default()
        .connection(Some(connection))
        .send_file(&copy.file.as_fd())
        .await;
    classify(request.and_then(|request| request.response()))
}

/// Falls back only on errors that show the portal is not there, which arise
/// before any request exists. An answer, or an error that could follow an
/// accepted request, ends the attempt.
pub(super) fn classify(result: Result<(), ashpd::Error>) -> Portal {
    match result {
        Ok(()) => Portal::Opened,
        Err(ashpd::Error::Response(ResponseError::Cancelled))
        | Err(ashpd::Error::Portal(PortalError::Cancelled(_))) => Portal::Cancelled,
        Err(ashpd::Error::Response(ResponseError::Other)) => {
            Portal::Refused("it could not open the copy".into())
        }
        Err(ashpd::Error::PortalNotFound(_)) => Portal::Unavailable,
        Err(ashpd::Error::Zbus(error)) | Err(ashpd::Error::Portal(PortalError::ZBus(error)))
            if absent(&error) =>
        {
            Portal::Unavailable
        }
        Err(ashpd::Error::Portal(PortalError::ZBus(error @ zbus::Error::MethodError(..)))) => {
            Portal::Refused(error.to_string())
        }
        Err(ashpd::Error::Portal(PortalError::ZBus(error))) => Portal::NoAnswer(error.to_string()),
        Err(ashpd::Error::Portal(error)) => Portal::Refused(error.to_string()),
        Err(error) => Portal::NoAnswer(error.to_string()),
    }
}

/// The bus has no portal service, it cannot be started, or it lacks OpenURI.
fn absent(error: &zbus::Error) -> bool {
    use zbus::fdo::Error as Fdo;
    match error {
        zbus::Error::InterfaceNotFound => true,
        zbus::Error::MethodError(name, ..) => {
            let name = name.as_str();
            matches!(
                name,
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.UnknownMethod"
                    | "org.freedesktop.DBus.Error.UnknownInterface"
                    | "org.freedesktop.DBus.Error.UnknownObject"
            ) || name.starts_with("org.freedesktop.DBus.Error.Spawn.")
        }
        zbus::Error::FDO(error) => matches!(
            **error,
            Fdo::ServiceUnknown(_)
                | Fdo::NameHasNoOwner(_)
                | Fdo::UnknownMethod(_)
                | Fdo::UnknownInterface(_)
                | Fdo::UnknownObject(_)
                | Fdo::SpawnExecFailed(_)
                | Fdo::SpawnForkFailed(_)
                | Fdo::SpawnChildExited(_)
                | Fdo::SpawnChildSignaled(_)
                | Fdo::SpawnFailed(_)
                | Fdo::SpawnServiceNotFound(_)
                | Fdo::SpawnServiceNotValid(_)
        ),
        _ => false,
    }
}

/// The first candidate that is an absolute path to an executable regular file.
pub(super) fn find_launcher(candidates: &[&Path]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    candidates
        .iter()
        .filter(|candidate| candidate.is_absolute())
        .find(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .map(|candidate| candidate.to_path_buf())
}

/// Runs a blocking check on its own thread, waiting for it at most `wait`;
/// a check that has not answered by then opens nothing.
async fn off_thread(
    executor: &BackgroundExecutor,
    wait: Duration,
    tool: &'static str,
    check: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (sender, checked) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check());
    });
    let checked = checked.fuse();
    let waited = executor.timer(wait).fuse();
    futures::pin_mut!(checked, waited);
    futures::select! {
        checked = checked => checked.unwrap_or_else(|_| {
            Err(format!("{tool}'s answer was lost, so nothing was opened."))
        }),
        () = waited => Err(format!("{tool} did not answer in time, so nothing was opened.")),
    }
}

/// Asks GIO what the copy is, as the portal will before it picks the
/// application. GIO takes the type from the suffix only when one type alone
/// claims it; for a suffix that several claim (`*.png` by PNG and APNG,
/// `*.ogg` by six types) it sniffs the content, where magic of priority 80 or
/// more wins over the leading signature. A PNG whose text chunk holds an SVG
/// document is then `image/svg+xml`, which a browser opens with its script.
/// The copy goes to the portal only when GIO's answer is one of the kind's
/// media types. `GIO_USE_VFS=local` keeps the query off D-Bus and gvfs; the
/// local file information it reads is the same.
pub(super) fn gio_agrees(
    kind: Kind,
    gio: &Path,
    copy: &Path,
    directory: &Path,
    mut environment: Vec<(OsString, OsString)>,
    wait: Duration,
) -> Result<(), String> {
    environment.push(("GIO_USE_VFS".into(), "local".into()));
    let arguments = [
        "info".into(),
        "-a".into(),
        "standard::content-type".into(),
        copy.as_os_str().to_owned(),
    ];
    let answer = query("gio", gio, &arguments, directory, environment, wait)?;
    let content_type = answer
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("standard::content-type:"))
        .unwrap_or_default()
        .trim();
    if !kind.media_types().contains(&content_type) {
        return Err(format!(
            "The desktop would open this copy as {} rather than as {}, so nothing was opened.",
            shown(content_type),
            kind.description()
        ));
    }
    Ok(())
}

/// Checks what `xdg-open` would make of the copy before it runs. The type
/// `xdg-mime` reports must be one `kind` may be: a sniffer can type a
/// polyglot by a later signature, such as an ISO 9660 descriptor in an Ogg
/// stream, and hand it to that type's application. The type must also have
/// a default application, since without one `xdg-open` goes on to mailcap,
/// `mimeopen` and web browsers. Both queries run as the launcher would,
/// each within `wait`.
pub(super) fn xdg_open_agrees(
    kind: Kind,
    xdg_mime: &Path,
    copy: &Path,
    directory: &Path,
    environment: Vec<(OsString, OsString)>,
    wait: Duration,
) -> Result<(), String> {
    let filetype = [
        "query".into(),
        "filetype".into(),
        copy.as_os_str().to_owned(),
    ];
    let answer = query(
        "xdg-mime",
        xdg_mime,
        &filetype,
        directory,
        environment.clone(),
        wait,
    )?;
    let first = answer.lines().next().unwrap_or_default();
    let media_type = first.split(';').next().unwrap_or_default().trim();
    if !kind.media_types().contains(&media_type) {
        return Err(format!(
            "xdg-open would open this copy as {} rather than as {}, so nothing was opened; the desktop portal (xdg-desktop-portal) opens it by its type.",
            shown(media_type),
            kind.description()
        ));
    }
    let default = ["query".into(), "default".into(), media_type.into()];
    let application = query("xdg-mime", xdg_mime, &default, directory, environment, wait)?;
    if !application
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .ends_with(".desktop")
    {
        return Err(format!(
            "No default application is set for {media_type}, so nothing was opened."
        ));
    }
    Ok(())
}

/// A sniffer's answer as the notice may show it.
fn shown(media_type: &str) -> String {
    let shown: String = media_type
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "/.+-_".contains(*c))
        .take(96)
        .collect();
    if shown.is_empty() {
        "an unknown type".into()
    } else {
        shown
    }
}

/// What `tool` prints for `arguments`, read up to [`MAX_ANSWER`] bytes once
/// it exits successfully within `wait`. It runs with no shell, no standard
/// input or error, the copy's folder as working directory, the launcher's
/// environment and its own process group, which is killed whole when the
/// wait runs out.
fn query(
    tool: &str,
    program: &Path,
    arguments: &[OsString],
    directory: &Path,
    environment: Vec<(OsString, OsString)>,
    wait: Duration,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .envs(session(environment))
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{tool} did not start: {error}."))?;
    let deadline = Instant::now() + wait;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                // SAFETY: the child is not yet reaped, so its process group
                // id still names the group it leads.
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.wait();
                return Err(format!(
                    "{tool} did not answer in time, so nothing was opened."
                ));
            }
            Err(error) => return Err(format!("{tool} could not be waited for: {error}.")),
        }
    };
    if !status.success() {
        return Err(format!(
            "{tool} could not type the copy, so nothing was opened."
        ));
    }
    let mut answer = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| format!("{tool} gave no answer, so nothing was opened."))?
        .take(MAX_ANSWER)
        .read_to_end(&mut answer)
        .map_err(|error| format!("{tool}'s answer could not be read: {error}."))?;
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

/// The session environment without any `GIT_` variable.
fn session(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> impl Iterator<Item = (OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(name, _)| !name.as_encoded_bytes().starts_with(b"GIT_"))
}

/// `program` with the copy's path as its one argument and no shell. It runs
/// in the copy's private folder, so an application that reads configuration
/// from its working directory (an editor's `.exrc`, say) never finds a
/// repository's; in its own process group, so the app's terminal signals do
/// not reach it; and with no inherited standard streams.
///
/// Its environment is exactly `environment` (the session's, which the
/// launcher needs to find the desktop and its handlers) without any `GIT_`
/// variable: `GIT_DIR`, `GIT_WORK_TREE` or `GIT_CONFIG_*` inherited from how
/// GitTurtle was started would point an unrelated application's Git
/// integration at a repository and its configured commands.
pub(super) fn launcher_command(
    program: &Path,
    copy: &Path,
    directory: &Path,
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Command {
    let mut command = Command::new(program);
    command
        .arg(copy)
        .env_clear()
        .envs(session(environment))
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

/// `xdg-open`'s documented exit codes, for one that ends within the wait.
fn launcher_result(status: ExitStatus) -> Result<(), String> {
    match status.code() {
        Some(0) => Ok(()),
        Some(2) => Err("xdg-open could not find the copy.".into()),
        Some(3) => Err("xdg-open found no application for this type.".into()),
        Some(4) => Err("The default application could not open the copy.".into()),
        Some(code) => Err(format!("xdg-open ended with status {code}.")),
        None => Err("xdg-open was stopped by a signal.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn only_a_missing_portal_falls_back_to_xdg_open() {
        let method_error = |name: &str| {
            let message = zbus::Message::method_call("/org/freedesktop/portal/desktop", "OpenFile")
                .unwrap()
                .build(&())
                .unwrap();
            zbus::Error::MethodError(name.try_into().unwrap(), None, message)
        };
        for unavailable in [
            ashpd::Error::PortalNotFound("org.freedesktop.portal.OpenURI".try_into().unwrap()),
            ashpd::Error::Portal(PortalError::ZBus(method_error(
                "org.freedesktop.DBus.Error.ServiceUnknown",
            ))),
            ashpd::Error::Portal(PortalError::ZBus(method_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
            ))),
            ashpd::Error::Zbus(zbus::Error::FDO(Box::new(
                zbus::fdo::Error::ServiceUnknown(String::new()),
            ))),
            ashpd::Error::Zbus(method_error(
                "org.freedesktop.DBus.Error.Spawn.ServiceNotFound",
            )),
        ] {
            assert_eq!(classify(Err(unavailable)), Portal::Unavailable);
        }
        assert_eq!(classify(Ok(())), Portal::Opened);
        // An answer, and anything that could follow an accepted request,
        // ends the attempt instead of opening the copy a second way.
        assert_eq!(
            classify(Err(ashpd::Error::Response(ResponseError::Cancelled))),
            Portal::Cancelled
        );
        assert!(matches!(
            classify(Err(ashpd::Error::Response(ResponseError::Other))),
            Portal::Refused(_)
        ));
        assert!(matches!(
            classify(Err(ashpd::Error::Portal(PortalError::NotAllowed(
                "no".into()
            )))),
            Portal::Refused(_)
        ));
        assert!(matches!(
            classify(Err(ashpd::Error::Portal(PortalError::ZBus(method_error(
                "org.freedesktop.DBus.Error.AccessDenied"
            ))))),
            Portal::Refused(_)
        ));
        assert!(matches!(
            classify(Err(ashpd::Error::NoResponse)),
            Portal::NoAnswer(_)
        ));
        assert!(matches!(
            classify(Err(ashpd::Error::Zbus(zbus::Error::InputOutput(
                std::io::Error::other("connection reset").into()
            )))),
            Portal::NoAnswer(_)
        ));
    }

    #[test]
    fn the_launcher_is_found_only_at_fixed_absolute_executable_paths() {
        let folder = tempfile::tempdir().unwrap();
        let plain = folder.path().join("plain");
        std::fs::write(&plain, "#!/bin/sh\n").unwrap();
        let program = folder.path().join("xdg-open");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let candidates = [
            Path::new("xdg-open"),
            folder.path(),
            plain.as_path(),
            program.as_path(),
        ];
        assert_eq!(find_launcher(&candidates), Some(program.clone()));
        assert_eq!(find_launcher(&candidates[..3]), None);
        assert!(XDG_OPEN.iter().all(|path| Path::new(path).is_absolute()));
    }

    /// A stand-in launcher records what it was given; the real spawn path
    /// runs it. The copy's path is full of shell syntax, which a shell would
    /// split, expand or execute, and the session environment it is given
    /// names a repository through Git's variables, which it never sees.
    #[test]
    fn the_launcher_gets_the_path_as_one_argument_without_a_shell_or_git_variables() {
        let folder = tempfile::tempdir().unwrap();
        let record = folder.path().join("record");
        let program = folder.path().join("launcher");
        std::fs::write(
            &program,
            // The streams are read before anything is redirected to the record.
            format!(
                "#!/bin/sh\nstreams=\"$(readlink /proc/$$/fd/0 /proc/$$/fd/1 /proc/$$/fd/2)\"\n\
                 {{ echo \"$#\"; printf '%s\\n' \"$1\"; pwd; \
                 echo \"$$ $(cut -d' ' -f5 /proc/$$/stat)\"; \
                 printf '%s\\n' \"$streams\"; \
                 echo \"${{GIT_DIR-unset}} ${{GIT_CONFIG_GLOBAL-unset}} ${{WAYLAND_DISPLAY-unset}}\"; }} > '{}'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let directory = folder.path().join("copy dir");
        std::fs::create_dir(&directory).unwrap();
        let canary = folder.path().join("ran");
        let copy = directory.join(format!(
            "Captured $(touch {canary}); revision `id` & ' \" *.txt",
            canary = canary.display()
        ));
        let environment = [
            (
                "PATH",
                std::env::var_os("PATH").unwrap_or("/usr/bin:/bin".into()),
            ),
            ("WAYLAND_DISPLAY", "wayland-1".into()),
            ("GIT_DIR", "/repository/.git".into()),
            ("GIT_CONFIG_GLOBAL", "/repository/config".into()),
        ]
        .map(|(name, value)| (OsString::from(name), value));
        let status = loop {
            match launcher_command(&program, &copy, &directory, environment.clone()).status() {
                Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                status => break status.unwrap(),
            }
        };
        assert!(status.success());
        let record = std::fs::read_to_string(&record).unwrap();
        let lines: Vec<&str> = record.lines().collect();
        assert_eq!(lines[0], "1", "exactly one argument");
        assert_eq!(lines[1], copy.to_str().unwrap());
        assert_eq!(Path::new(lines[2]), directory);
        let (pid, group) = lines[3].split_once(' ').unwrap();
        assert_eq!(pid, group, "the launcher leads its own process group");
        assert_eq!(&lines[4..7], ["/dev/null"; 3]);
        assert_eq!(
            lines[7], "unset unset wayland-1",
            "Git's variables are removed"
        );
        assert!(!canary.exists(), "nothing in the path ran");
    }

    /// Runs `attempt` again while a stand-in a test has just written cannot
    /// be executed yet: a child that another test thread forked while the
    /// script was open for writing keeps it busy (`ETXTBSY`) until that child
    /// executes its own program. Only programs written by the tests meet it.
    fn past_busy<T>(mut attempt: impl FnMut() -> Result<T, String>) -> Result<T, String> {
        for _ in 0..200 {
            match attempt() {
                Err(error) if error.contains("os error 26") => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => return result,
            }
        }
        attempt()
    }

    /// A stand-in `xdg-mime` that answers from its environment, as the real
    /// one answers from the copy's content, and records each call.
    fn stand_in_xdg_mime(folder: &Path) -> (PathBuf, PathBuf) {
        let (program, record) = (folder.join("xdg-mime"), folder.join("queries"));
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"$#|$1|$2|$3|${GIT_DIR-unset}\" >> \"$RECORD\"\n\
             [ -n \"$SLEEP\" ] && sleep \"$SLEEP\"\n\
             case \"$2\" in filetype) printf '%s\\n' \"$TYPE\";; default) printf '%s\\n' \"$DEFAULT\";; esac\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (program, record)
    }

    fn answers(record: &Path, pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        let path = std::env::var_os("PATH").unwrap_or("/usr/bin:/bin".into());
        [
            ("PATH", path),
            ("RECORD", record.into()),
            ("GIT_DIR", "/repository/.git".into()),
        ]
        .into_iter()
        .chain(
            pairs
                .iter()
                .map(|(name, value)| (*name, OsString::from(value))),
        )
        .map(|(name, value)| (OsString::from(name), value))
        .collect()
    }

    /// xdg-open types the copy with xdg-mime, which sniffs content: a
    /// polyglot that a sniffer types as something else, an untyped copy and
    /// a type with no default application (which xdg-open would hand on to
    /// mailcap or a browser) all open nothing.
    #[test]
    fn xdg_open_runs_only_when_xdg_mime_agrees_with_the_content() {
        let folder = tempfile::tempdir().unwrap();
        let (program, record) = stand_in_xdg_mime(folder.path());
        let copy = folder.path().join("Captured revision.png");
        let wait = Duration::from_secs(5);
        let check = |kind: Kind, pairs: &[(&str, &str)]| {
            past_busy(|| {
                let environment = answers(&record, pairs);
                xdg_open_agrees(kind, &program, &copy, folder.path(), environment, wait)
            })
        };
        let player = ("DEFAULT", "imv.desktop");
        assert_eq!(check(Kind::Png, &[("TYPE", "image/png"), player]), Ok(()));
        let queries = std::fs::read_to_string(&record).unwrap();
        assert_eq!(
            queries.lines().collect::<Vec<_>>(),
            [
                format!("3|query|filetype|{}|unset", copy.display()),
                "3|query|default|image/png|unset".into(),
            ],
            "one argument each, and no Git variables"
        );
        let charset = ("TYPE", "image/png; charset=binary");
        assert_eq!(check(Kind::Png, &[charset, player]), Ok(()));
        let disk = check(
            Kind::Png,
            &[("TYPE", "application/x-iso9660-image"), player],
        );
        let refused = disk.unwrap_err();
        assert!(refused.contains("as application/x-iso9660-image rather than as a PNG image"));
        let untyped = check(Kind::Pdf, &[("TYPE", "application/octet-stream"), player]);
        assert!(untyped.unwrap_err().contains("application/octet-stream"));
        assert!(
            check(Kind::Png, &[("TYPE", "<b>")])
                .unwrap_err()
                .contains(" as b rather")
        );
        let unhandled = check(Kind::Png, &[("TYPE", "image/png"), ("DEFAULT", "")]);
        assert!(unhandled.unwrap_err().starts_with("No default application"));
        let start = Instant::now();
        let slow = [("TYPE", "image/png"), player, ("SLEEP", "10")];
        let environment = answers(&record, &slow);
        let late = xdg_open_agrees(
            Kind::Png,
            &program,
            &copy,
            folder.path(),
            environment,
            Duration::from_millis(300),
        );
        assert!(late.unwrap_err().contains("did not answer in time"));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the group was killed"
        );
    }

    #[test]
    fn every_kind_names_the_media_types_it_may_be_opened_as() {
        for kind in [
            Kind::Png,
            Kind::Jpeg,
            Kind::Gif,
            Kind::WebP,
            Kind::Bmp,
            Kind::Tiff,
            Kind::Icon,
            Kind::Avif,
            Kind::Heif,
            Kind::Jp2,
            Kind::J2k,
            Kind::Pdf,
        ] {
            assert!(kind.typed_by_signature());
            assert!(!kind.media_types().is_empty(), "{kind:?}");
            assert!(kind.media_types().iter().all(|name| name.contains('/')));
        }
        assert_eq!(Kind::Text.media_types(), ["text/plain"]);
    }

    /// A stand-in `gio` that prints what `gio info` prints, with the content
    /// type taken from its environment, and records each call.
    fn stand_in_gio(folder: &Path) -> (PathBuf, PathBuf) {
        let (program, record) = (folder.join("gio"), folder.join("queries"));
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"$#|$1|$2|$3|$4|${GIO_USE_VFS-unset}|${GIT_DIR-unset}\" >> \"$RECORD\"\n\
             [ -n \"$SLEEP\" ] && sleep \"$SLEEP\"\n\
             printf 'uri: file://%s\\nlocal path: %s\\nattributes:\\n' \"$4\" \"$4\"\n\
             [ -n \"$TYPE\" ] && printf '  standard::content-type: %s\\n' \"$TYPE\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (program, record)
    }

    /// The portal types a copy as GIO does, which sniffs the content when a
    /// suffix has several claimants: the copy goes on only when GIO's answer
    /// is one of the kind's media types.
    #[test]
    fn the_portal_gets_the_copy_only_when_gio_agrees_with_the_content() {
        let folder = tempfile::tempdir().unwrap();
        let (program, record) = stand_in_gio(folder.path());
        let copy = folder.path().join("Captured revision.png");
        let check = |kind: Kind, pairs: &[(&str, &str)], wait: Duration| {
            past_busy(|| {
                let environment = answers(&record, pairs);
                gio_agrees(kind, &program, &copy, folder.path(), environment, wait)
            })
        };
        let wait = Duration::from_secs(5);
        assert_eq!(check(Kind::Png, &[("TYPE", "image/png")], wait), Ok(()));
        assert_eq!(
            std::fs::read_to_string(&record).unwrap().trim_end(),
            format!(
                "4|info|-a|standard::content-type|{}|local|unset",
                copy.display()
            ),
            "one path argument, the local VFS and no Git variables"
        );
        let svg = check(Kind::Png, &[("TYPE", "image/svg+xml")], wait).unwrap_err();
        assert_eq!(
            svg,
            "The desktop would open this copy as image/svg+xml rather than as a PNG image, so nothing was opened."
        );
        let php = check(Kind::Pdf, &[("TYPE", "application/x-php")], wait).unwrap_err();
        assert!(php.contains(" as application/x-php rather than as a PDF document"));
        assert_eq!(check(Kind::Text, &[("TYPE", "text/plain")], wait), Ok(()));
        let untyped = check(Kind::Png, &[], wait).unwrap_err();
        assert!(untyped.contains(" as an unknown type "));
        let start = Instant::now();
        let slow = [("TYPE", "image/png"), ("SLEEP", "10")];
        let late = check(Kind::Png, &slow, Duration::from_millis(300)).unwrap_err();
        assert!(late.contains("did not answer in time"));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the wait is bounded"
        );
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }

    /// A 1 × 1 PNG, with `text` in a tEXt chunk when given.
    fn png(text: Option<&[u8]>) -> Vec<u8> {
        let chunk = |kind: &[u8], data: &[u8]| {
            let body = [kind, data].concat();
            [
                &(data.len() as u32).to_be_bytes()[..],
                &body,
                &crc32(&body).to_be_bytes(),
            ]
            .concat()
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", b"\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0"));
        if let Some(text) = text {
            png.extend(chunk(b"tEXt", &[b"Comment\0".as_slice(), text].concat()));
        }
        let pixels = b"\x78\x01\x01\x05\0\xfa\xff\0\0\0\0\0\0\x05\0\x01";
        png.extend(chunk(b"IDAT", pixels));
        png.extend(chunk(b"IEND", b""));
        png
    }

    /// The real `gio`, where it is installed, answers as the portal will. A
    /// plain PNG agrees, and each polyglot, a PNG whose text chunk carries an
    /// SVG document or a PHP opening tag, is refused exactly when GIO types
    /// it as something else, as GLib with shared-mime-info 2.2 or later does
    /// (`*.png` is also claimed by APNG there, so GIO sniffs and SVG's
    /// priority 80 magic wins).
    #[test]
    fn the_real_gio_decides_for_polyglots() {
        let Some(gio) = find_launcher(&GIO.map(Path::new)) else {
            return;
        };
        let svg = br#"<!DOCTYPE svg><svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;
        let folder = tempfile::tempdir().unwrap();
        let environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let samples = [
            ("plain", Kind::Png, png(None)),
            ("png-svg", Kind::Png, png(Some(svg))),
            (
                "png-php",
                Kind::Png,
                png(Some(b"<?php system($_GET[0]); ?>")),
            ),
        ];
        for (name, kind, bytes) in samples {
            let directory = folder.path().join(name);
            std::fs::create_dir(&directory).unwrap();
            let copy = directory.join(format!("Captured revision.{}", kind.extension()));
            std::fs::write(&copy, bytes).unwrap();
            let wait = Duration::from_secs(5);
            let verdict = gio_agrees(kind, &gio, &copy, &directory, environment.clone(), wait);
            let mut asked = environment.clone();
            asked.push(("GIO_USE_VFS".into(), "local".into()));
            let arguments = [
                "info".into(),
                "-a".into(),
                "standard::content-type".into(),
                copy.clone().into_os_string(),
            ];
            let answer = query("gio", &gio, &arguments, &directory, asked, wait).unwrap();
            let typed = answer
                .lines()
                .find_map(|line| line.trim_start().strip_prefix("standard::content-type:"))
                .unwrap()
                .trim()
                .to_owned();
            assert_eq!(
                verdict.is_ok(),
                kind.media_types().contains(&typed.as_str()),
                "{name}: GIO says {typed}, the check says {verdict:?}"
            );
            if let Err(refusal) = verdict {
                assert!(refusal.contains(&typed), "{name}: {refusal}");
            }
            if name == "plain" {
                assert_eq!(typed, "image/png");
            }
        }
    }

    #[test]
    fn early_launcher_exits_name_the_failure() {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(launcher_result(ExitStatus::from_raw(0)), Ok(()));
        assert!(
            launcher_result(ExitStatus::from_raw(3 << 8))
                .unwrap_err()
                .contains("no application")
        );
        assert!(launcher_result(ExitStatus::from_raw(9)).is_err());
    }
}
