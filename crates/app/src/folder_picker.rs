//! Normalize native picker replies without confusing cancellation and failure.
//!
//! Every native file dialog answers the same three ways: a path, a quiet
//! cancellation, and a service failure that needs the Linux portal guidance.
//! The wording names what was being chosen, and only the repository picker can
//! suggest the command line as the way around a broken portal.
use futures::channel::oneshot;
use std::path::PathBuf;

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
            Self::Folder => "Could not choose a folder",
            Self::File => "Could not choose a file",
            Self::Destination => "Could not choose where to save",
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
        Ok(Err(error)) => Err(guidance(format!("{}: {error:#}", picker.failure()), picker)),
        Err(_) => Err(guidance(picker.closed().into(), picker)),
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
        Ok(Err(error)) => Err(guidance(format!("{}: {error:#}", picker.failure()), picker)),
        Err(_) => Err(guidance(picker.closed().into(), picker)),
    }
}

/// A picker service failure is actionable on Linux, where a missing portal is
/// the usual cause.
fn guidance(message: String, picker: Picker) -> String {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = picker;
        message
    }
    #[cfg(target_os = "linux")]
    {
        let repository = match picker {
            Picker::Folder => " You can also open a repository by passing its path to GitTurtle.",
            Picker::File | Picker::Destination => "",
        };
        format!(
            "{message}\n\nCheck that xdg-desktop-portal and a FileChooser backend are running in your desktop session.{repository}"
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
        assert!(error.contains("D-Bus request failed: FileChooser service unavailable"));
        #[cfg(target_os = "linux")]
        assert!(error.contains("xdg-desktop-portal"));

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
        assert!(error.starts_with("Could not choose where to save: "));

        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(anyhow::anyhow!("FileChooser service unavailable")))
            .unwrap();
        let file_error =
            futures::executor::block_on(selected_path_for(Picker::File, receiver)).unwrap_err();
        assert!(file_error.starts_with("Could not choose a file: "));

        let (sender, receiver) = oneshot::channel();
        drop(sender);
        let closed = futures::executor::block_on(new_path(receiver)).unwrap_err();
        assert!(closed.starts_with("The file picker closed unexpectedly."));

        #[cfg(target_os = "linux")]
        for message in [&error, &file_error, &closed] {
            assert!(message.contains("xdg-desktop-portal"), "{message}");
            assert!(
                !message.contains("passing its path to GitTurtle"),
                "only the repository picker suggests the command line: {message}"
            );
        }
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
