//! Coalesced wakeups keep Objective-C notifications outside GPUI's App borrow.
#[cfg(target_os = "macos")]
use crate::gpui;
use futures::channel::mpsc;
use std::sync::Mutex;

struct Signal(Mutex<mpsc::Sender<()>>);
impl Signal {
    fn send(&self) {
        // One sender owns one reserved channel slot. Full means a wakeup is
        // already pending; closed means the owning view was released.
        let _ = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_send(());
    }
}
fn channel() -> (Signal, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel(0);
    (Signal(Mutex::new(sender)), receiver)
}

#[cfg(target_os = "macos")]
pub(super) fn subscribe(
    window: &gpui::Window,
    cx: &mut gpui::Context<crate::GitTurtle>,
) -> gpui::Task<()> {
    use block2::RcBlock;
    use futures::{FutureExt as _, StreamExt as _};
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{
        NSSystemColorsDidChangeNotification, NSWorkspace,
        NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    };
    use objc2_foundation::{
        NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue,
    };
    use std::ptr::NonNull;

    struct Observer {
        center: Retained<NSNotificationCenter>,
        token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }
    impl Drop for Observer {
        fn drop(&mut self) {
            let token: &ProtocolObject<dyn NSObjectProtocol> = &self.token;
            // SAFETY: this is the token registered with this retained center.
            unsafe { self.center.removeObserver(token.as_ref()) };
        }
    }

    let (signal, mut changes) = channel();
    let workspace = NSWorkspace::sharedWorkspace();
    let workspace_center = workspace.notificationCenter();
    let display_signal = signal;
    let (accent_signal, mut accent_changes) = channel();
    let callback = RcBlock::new(move |_: NonNull<NSNotification>| display_signal.send());
    // Apple's accessibility display notification is delivered through NSWorkspace's center.
    // SAFETY: the notification's object is NSWorkspace, delivery uses the main
    // queue, and the block captures only a Send + Sync bounded-channel signal.
    let token = unsafe {
        workspace_center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            Some(&workspace),
            Some(&NSOperationQueue::mainQueue()),
            &callback,
        )
    };
    let observer = Observer {
        center: workspace_center,
        token,
    };

    let default_center = NSNotificationCenter::defaultCenter();
    let accent_callback = RcBlock::new(move |_: NonNull<NSNotification>| accent_signal.send());
    // System accent changes are posted on NSNotificationCenter::defaultCenter() with nil object.
    // SAFETY: delivery uses the main queue and the block captures only a bounded-channel signal.
    let accent_token = unsafe {
        default_center.addObserverForName_object_queue_usingBlock(
            Some(NSSystemColorsDidChangeNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &accent_callback,
        )
    };
    let accent_observer = Observer {
        center: default_center,
        token: accent_token,
    };

    cx.spawn_in(window, async move |view, cx| {
        let _observer = observer;
        let _accent_observer = accent_observer;
        loop {
            futures::select! {
                display_event = changes.next().fuse() => {
                    if display_event.is_none() {
                        break;
                    }
                    // Reading current values here coalesces rapid ON/OFF transitions.
                    // Never update GPUI synchronously inside a Cocoa notification.
                    if view
                        .update_in(cx, |view, window, cx| {
                            super::sync_preferences(cx);
                            view.show_display_preferences(None, window, cx);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                accent_event = accent_changes.next().fuse() => {
                    if accent_event.is_none() {
                        break;
                    }
                    if view
                        .update_in(cx, |view, window, cx| {
                            crate::appearance::sync_macos_system_accent(cx);
                            if view.settings.system_accent {
                                view.apply_appearance(window, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_change_bursts_coalesce_and_closed_views_accept_no_work() {
        let (signal, mut receiver) = channel();
        for _ in 0..1000 {
            signal.send();
        }
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        signal.send();
        assert_eq!(receiver.try_recv(), Ok(()));
        drop(receiver);
        signal.send();
    }
}
