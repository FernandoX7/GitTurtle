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
pub(super) fn subscribe(cx: &mut gpui::Context<crate::GitTurtle>) -> gpui::Task<()> {
    use block2::RcBlock;
    use futures::StreamExt;
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
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
            // SAFETY: this is the token registered with this retained center.
            let token: &ProtocolObject<dyn NSObjectProtocol> = &self.token;
            unsafe { self.center.removeObserver(token.as_ref()) };
        }
    }

    let (signal, mut changes) = channel();
    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();
    let callback = RcBlock::new(move |_: NonNull<NSNotification>| signal.send());
    // Apple's notification is delivered through NSWorkspace's own center.
    // SAFETY: the notification's object is NSWorkspace, delivery uses the main
    // queue, and the block captures only a Send + Sync bounded-channel signal.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            Some(&workspace),
            Some(&NSOperationQueue::mainQueue()),
            &callback,
        )
    };
    let observer = Observer { center, token };
    cx.spawn(async move |view, cx| {
        let _observer = observer;
        while changes.next().await.is_some() {
            // Reading current values here coalesces rapid ON/OFF transitions.
            // Never update GPUI synchronously inside a Cocoa notification.
            if view
                .update(cx, |view, cx| {
                    super::sync_preferences(cx);
                    view.apply_motion_preferences(cx);
                    cx.notify();
                })
                .is_err()
            {
                break;
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
