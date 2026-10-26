//! Follows the user's fontconfig configuration while the app runs, so that a
//! desktop font change, such as `omarchy font set` rewriting
//! `~/.config/fontconfig/fonts.conf`, reaches code text and the fontconfig
//! rendering defaults at once rather than when focus next returns. The
//! `fontconfig/` directory need not exist at launch: its parent is watched for
//! it, and the directory is watched again after each change, so one that is
//! created, removed or replaced keeps being followed. Reads, such as
//! `fc-match` opening `fonts.conf`, are not changes, so a refresh never asks
//! for another.

use futures::{StreamExt as _, channel::mpsc};
use gpui_kit::BackgroundExecutor;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// A rewrite settles when this passes without another write, as the Omarchy
/// theme watch waits for a theme switch.
const QUIET_PERIOD: Duration = Duration::from_millis(250);

/// Where fontconfig reads the user's `fontconfig/`: `$XDG_CONFIG_HOME`, or
/// `~/.config` when that is unset or not absolute.
pub(super) fn config_home() -> Option<PathBuf> {
    let absolute = |value: std::ffi::OsString| {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    };
    std::env::var_os("XDG_CONFIG_HOME")
        .and_then(absolute)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|home| home.join(".config"))
        })
}

/// Sends `refresh` a request for a new desktop-text read each time the
/// configuration under `config_home` settles after a change. Never returns:
/// when no watch can start, or `refresh` closes, it waits to be dropped with
/// the observer it serves, and focus return still refreshes.
pub(super) async fn follow(
    config_home: PathBuf,
    mut refresh: mpsc::Sender<()>,
    executor: BackgroundExecutor,
) {
    let (events, mut changes) = mpsc::unbounded();
    if let Ok(mut watch) = Watch::start(&config_home, events) {
        while changes.next().await.is_some() {
            loop {
                executor.timer(QUIET_PERIOD).await;
                let mut more = false;
                while changes.try_recv().is_ok() {
                    more = true;
                }
                if !more {
                    break;
                }
            }
            watch.arm();
            // A refresh already queued reads this change too.
            if refresh.try_send(()).is_err() && refresh.is_closed() {
                break;
            }
        }
    }
    futures::future::pending::<()>().await
}

struct Watch {
    watcher: notify::RecommendedWatcher,
    directory: PathBuf,
    watching: bool,
}

impl Watch {
    fn start(config_home: &Path, events: mpsc::UnboundedSender<()>) -> notify::Result<Self> {
        use notify::Watcher as _;
        let directory = config_home.join("fontconfig");
        let followed = directory.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                // An error may have lost events: read again to be sure.
                if event
                    .as_ref()
                    .map_or(true, |event| changes(&followed, event))
                {
                    let _ = events.unbounded_send(());
                }
            })?;
        watcher.watch(config_home, notify::RecursiveMode::NonRecursive)?;
        let mut watch = Self {
            watcher,
            directory,
            watching: false,
        };
        watch.arm();
        Ok(watch)
    }

    /// Watches `fontconfig/` afresh when it exists, so a directory created or
    /// replaced since the last change is the one followed.
    fn arm(&mut self) {
        use notify::Watcher as _;
        if self.watching {
            let _ = self.watcher.unwatch(&self.directory);
        }
        self.watching = self
            .watcher
            .watch(&self.directory, notify::RecursiveMode::Recursive)
            .is_ok();
    }
}

/// Whether `event` can change the configuration in `directory`: a write,
/// creation, removal or rename in it, or of it in its parent. Reads and
/// unrelated entries of the parent cannot.
fn changes(directory: &Path, event: &notify::Event) -> bool {
    !matches!(event.kind, notify::EventKind::Access(_))
        && (event.need_rescan() || event.paths.iter().any(|path| path.starts_with(directory)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{
        Event, EventKind,
        event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind},
    };

    #[test]
    fn only_changes_to_the_fontconfig_directory_count() {
        let directory = Path::new("/home/user/.config/fontconfig");
        let event = |kind, path: &str| Event::new(kind).add_path(path.into());
        let write = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        assert!(changes(
            directory,
            &event(
                EventKind::Create(CreateKind::Folder),
                "/home/user/.config/fontconfig"
            )
        ));
        assert!(changes(
            directory,
            &event(write, "/home/user/.config/fontconfig/fonts.conf")
        ));
        assert!(changes(
            directory,
            &event(write, "/home/user/.config/fontconfig/conf.d/50-mono.conf")
        ));
        // `fc-match` opening and reading the file it watches.
        for access in [AccessKind::Open(AccessMode::Read), AccessKind::Read] {
            assert!(!changes(
                directory,
                &event(
                    EventKind::Access(access),
                    "/home/user/.config/fontconfig/fonts.conf"
                )
            ));
        }
        // Siblings in `~/.config`, including one sharing the name's prefix.
        for sibling in [
            "/home/user/.config/mimeapps.list",
            "/home/user/.config/fontconfig-old",
        ] {
            assert!(!changes(directory, &event(write, sibling)));
        }
    }

    /// A real watch on a configuration home without `fontconfig/`: creating
    /// it with `fonts.conf`, as `omarchy font set` does, asks for a refresh,
    /// and so does each later rewrite, once armed again, but a read does not.
    #[test]
    fn a_created_directory_is_followed_through_later_rewrites() {
        let home = tempfile::tempdir().unwrap();
        let (events, mut changes) = mpsc::unbounded();
        let mut watch = Watch::start(home.path(), events).unwrap();
        assert!(!watch.watching);
        let next = |changes: &mut mpsc::UnboundedReceiver<()>| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if changes.try_recv().is_ok() {
                    return true;
                }
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let settle = |changes: &mut mpsc::UnboundedReceiver<()>| {
            std::thread::sleep(QUIET_PERIOD);
            while changes.try_recv().is_ok() {}
        };

        let fonts = home.path().join("fontconfig/fonts.conf");
        std::fs::create_dir(home.path().join("fontconfig")).unwrap();
        std::fs::write(&fonts, "<fontconfig/>").unwrap();
        assert!(next(&mut changes), "creating fontconfig/ is a change");
        settle(&mut changes);
        watch.arm();
        assert!(watch.watching);

        std::fs::write(&fonts, "<fontconfig></fontconfig>").unwrap();
        assert!(next(&mut changes), "rewriting fonts.conf is a change");
        settle(&mut changes);
        watch.arm();

        std::fs::read(&fonts).unwrap();
        std::thread::sleep(QUIET_PERIOD);
        assert!(changes.try_recv().is_err(), "a read is not");
    }
}
