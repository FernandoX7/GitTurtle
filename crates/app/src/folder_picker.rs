//! Normalize native picker replies without confusing cancellation and failure.
use futures::channel::oneshot;
use std::path::PathBuf;

pub(super) async fn selected_path(
    response: oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>>,
) -> Result<Option<PathBuf>, String> {
    let result = match response.await {
        Ok(Ok(paths)) => return Ok(paths.and_then(|paths| paths.into_iter().next())),
        Ok(Err(error)) => format!("Could not choose a folder: {error:#}"),
        Err(_) => "The folder picker closed unexpectedly. Please try again.".into(),
    };
    #[cfg(target_os = "linux")]
    let result = format!(
        "{result}\n\nCheck that xdg-desktop-portal and a FileChooser backend are running in your desktop session. You can also open a repository by passing its path to GitTurtle."
    );
    Err(result)
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
