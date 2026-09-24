//! Normalize native picker replies without confusing cancellation and failure.
//!
//! Every native file dialog answers the same three ways: a path, a quiet
//! cancellation, and a service failure that needs the Linux portal guidance.
//! The wording names what was being chosen, and only the repository picker can
//! suggest the command line as the way around a broken portal. A failure's
//! first line is what the user can do; the service's own error follows on a
//! line of its own, shortened and drawn muted, since it names D-Bus internals.
use crate::{HighlightStyle, StyledText, rgb};
use futures::channel::oneshot;
use std::path::PathBuf;

/// The most characters of a service error a failure quotes.
const MAX_DETAIL_CHARS: usize = 160;

/// What introduces the service's own error in a failure.
const DETAIL: &str = "\nDetails: ";

/// What a native dialog was opened for, so its failure names that.
#[derive(Clone, Copy)]
pub(super) enum Picker {
    /// A repository or parent folder (`prompt_for_paths`, directories).
    Folder,
    /// An existing file to read, such as a theme document (`prompt_for_paths`).
    File,
    /// Where to write a new document (`prompt_for_new_path`).
    Destination,
}

impl Picker {
    fn failure(self) -> &'static str {
        match self {
            Self::Folder => "Could not choose a folder.",
            Self::File => "Could not choose a file.",
            Self::Destination => "Could not choose where to save.",
        }
    }

    fn closed(self) -> &'static str {
        match self {
            Self::Folder => "The folder picker closed unexpectedly. Please try again.",
            Self::File | Self::Destination => {
                "The file picker closed unexpectedly. Please try again."
            }
        }
    }
}

pub(super) async fn selected_path(
    response: oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>>,
) -> Result<Option<PathBuf>, String> {
    selected_path_for(Picker::Folder, response).await
}

/// One chosen path from a `prompt_for_paths` dialog.
pub(super) async fn selected_path_for(
    picker: Picker,
    response: oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>>,
) -> Result<Option<PathBuf>, String> {
    match response.await {
        Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
        Ok(Err(error)) => Err(failure(picker, &error)),
        Err(_) => Err(guidance(picker.closed(), picker)),
    }
}

/// The destination a `prompt_for_new_path` dialog chose, which is a single
/// path rather than a list.
pub(super) async fn new_path(
    response: oneshot::Receiver<anyhow::Result<Option<PathBuf>>>,
) -> Result<Option<PathBuf>, String> {
    let picker = Picker::Destination;
    match response.await {
        Ok(Ok(path)) => Ok(path),
        Ok(Err(error)) => Err(failure(picker, &error)),
        Err(_) => Err(guidance(picker.closed(), picker)),
    }
}

/// A service failure: the guidance line, then the error chain on one line
/// of its own, at most [`MAX_DETAIL_CHARS`] characters.
fn failure(picker: Picker, error: &anyhow::Error) -> String {
    let detail = format!("{error:#}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let detail = match detail.char_indices().nth(MAX_DETAIL_CHARS) {
        Some((end, _)) => format!("{}…", detail[..end].trim_end()),
        None => detail,
    };
    format!("{}{DETAIL}{detail}", guidance(picker.failure(), picker))
}

/// Where a failure's quoted service error starts: the line after the
/// guidance. Any other message has none.
pub(super) fn detail_start(message: &str) -> Option<usize> {
    message.find(DETAIL).map(|line_break| line_break + 1)
}

/// `message` as one text in its element's color, except a failure's quoted
/// service error, which is `muted`: secondary to the guidance above it. One
/// text keeps a line clamp and the accessible name on the whole message.
pub(super) fn styled_message(message: &str, muted: u32) -> StyledText {
    let color = Some(rgb(muted).into());
    StyledText::new(message.to_owned()).with_highlights(detail_start(message).map(|start| {
        let style = HighlightStyle {
            color,
            ..Default::default()
        };
        (start..message.len(), style)
    }))
}

/// A picker service failure is actionable on Linux, where a missing portal is
/// the usual cause.
fn guidance(message: &str, picker: Picker) -> String {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = picker;
        message.to_owned()
    }
    #[cfg(target_os = "linux")]
    {
        let repository = match picker {
            Picker::Folder => " You can also open a repository by passing its path to GitTurtle.",
            Picker::File | Picker::Destination => "",
        };
        format!(
            "{message} Check that xdg-desktop-portal and a FileChooser backend are running in your desktop session.{repository}"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_cancellation_is_quiet_but_service_failures_are_actionable() {
        for response in [None, Some(Vec::new())] {
            let (sender, receiver) = oneshot::channel();
            sender.send(Ok(response)).unwrap();
            assert_eq!(
                futures::executor::block_on(selected_path(receiver)),
                Ok(None)
            );
        }
        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(
                anyhow::anyhow!("FileChooser service unavailable").context("D-Bus request failed")
            ))
            .unwrap();
        let error = futures::executor::block_on(selected_path(receiver)).unwrap_err();
        // The first line is what the user can do; the chain follows it.
        let (first, detail) = error.split_once('\n').expect("the detail has its own line");
        assert!(first.starts_with("Could not choose a folder."), "{first}");
        assert!(!first.contains("D-Bus"), "{first}");
        assert_eq!(
            detail,
            "Details: D-Bus request failed: FileChooser service unavailable"
        );
        #[cfg(target_os = "linux")]
        assert!(
            first.contains("xdg-desktop-portal") && first.contains("passing its path to GitTurtle"),
            "{first}"
        );

        let (sender, receiver) = oneshot::channel();
        drop(sender);
        assert!(
            futures::executor::block_on(selected_path(receiver))
                .unwrap_err()
                .contains("closed unexpectedly")
        );
    }

    #[test]
    fn file_and_destination_pickers_share_the_portal_guidance_without_the_repository_hint() {
        let (sender, receiver) = oneshot::channel();
        sender.send(Ok(None)).unwrap();
        assert_eq!(futures::executor::block_on(new_path(receiver)), Ok(None));

        let (sender, receiver) = oneshot::channel();
        let chosen = PathBuf::from("/tmp/sunset.gitturtle-theme.json");
        sender.send(Ok(Some(chosen.clone()))).unwrap();
        assert_eq!(
            futures::executor::block_on(new_path(receiver)),
            Ok(Some(chosen))
        );

        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(anyhow::anyhow!("FileChooser service unavailable")))
            .unwrap();
        let error = futures::executor::block_on(new_path(receiver)).unwrap_err();
        assert!(error.starts_with("Could not choose where to save."));

        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(anyhow::anyhow!("FileChooser service unavailable")))
            .unwrap();
        let file_error =
            futures::executor::block_on(selected_path_for(Picker::File, receiver)).unwrap_err();
        assert!(file_error.starts_with("Could not choose a file."));

        let (sender, receiver) = oneshot::channel();
        drop(sender);
        let closed = futures::executor::block_on(new_path(receiver)).unwrap_err();
        assert!(closed.starts_with("The file picker closed unexpectedly."));

        #[cfg(target_os = "linux")]
        for message in [&error, &file_error, &closed] {
            let first = message.lines().next().unwrap_or_default();
            assert!(first.contains("xdg-desktop-portal"), "{message}");
            assert!(
                !message.contains("passing its path to GitTurtle"),
                "only the repository picker suggests the command line: {message}"
            );
        }
    }

    /// A service error is quoted after the guidance as one line, however
    /// many lines the chain has, and shortened with an ellipsis.
    #[test]
    fn a_long_service_error_is_one_shortened_line_after_the_guidance() {
        let (sender, receiver) = oneshot::channel();
        let name = "org.freedesktop.portal.Desktop ".repeat(12);
        sender
            .send(Err(anyhow::anyhow!(
                "The name {name}\nwas not provided by any .service files"
            )
            .context("Portal request failed")))
            .unwrap();
        let error =
            futures::executor::block_on(selected_path_for(Picker::File, receiver)).unwrap_err();
        let lines = error.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(lines[0].starts_with("Could not choose a file."), "{error}");
        let detail = lines[1]
            .strip_prefix("Details: ")
            .expect("the detail is marked as such");
        assert!(
            detail.starts_with("Portal request failed: The name org.freedesktop.portal.Desktop "),
            "{detail}"
        );
        assert!(detail.ends_with('…'), "{detail}");
        assert!(detail.chars().count() <= MAX_DETAIL_CHARS + 1, "{detail}");
    }

    #[cfg(unix)]
    #[test]
    fn chosen_folder_keeps_native_filename_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/repo-\xff".to_vec()));
        let (sender, receiver) = oneshot::channel();
        sender.send(Ok(Some(vec![path.clone()]))).unwrap();
        assert_eq!(
            futures::executor::block_on(selected_path(receiver)),
            Ok(Some(path))
        );
    }
}
