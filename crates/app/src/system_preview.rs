//! The explicit action that opens a read-only copy of captured revision bytes
//! in another application: Quick Look's System preview on macOS, and Open in
//! default app on Linux.
//!
//! Repository content controls both the bytes and the stored file name. Only a
//! click opens anything: selection, refresh, tab restore and reopening never
//! do. Whether a side offers the action is decided on the worker when the
//! side is prepared ([`offer`]), never while rendering. On Linux that decision
//! is made from content alone and also fixes the type the copy is handed over
//! as ([`passive`]); the repository's file name is never used. The copy lives
//! in a private per-user cache ([`store`]) and goes to the desktop through
//! the OpenURI portal, or `xdg-open` where no portal answers, only after the
//! desktop's own type sniffers (GIO, and `xdg-mime` for `xdg-open`) agree
//! with that type ([`launch`]).

use anyhow::Result;

#[cfg(target_os = "linux")]
mod launch;
#[cfg(target_os = "linux")]
pub(crate) mod passive;
#[cfg(target_os = "linux")]
mod store;

/// The open action one captured side offers. Sides that offer none hold
/// `None`, so no surface shows the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    #[cfg(target_os = "linux")]
    kind: passive::Kind,
}

/// Quick Look previews any captured bytes and never launches an application.
#[cfg(target_os = "macos")]
pub(crate) fn offer(_bytes: &[u8], _check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(Some(Offer {}))
}

/// The desktop's default application receives only a passive type that the
/// content itself establishes.
#[cfg(target_os = "linux")]
pub(crate) fn offer(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(passive::Kind::of(bytes, check)?.map(|kind| Offer { kind }))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn offer(_bytes: &[u8], _check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(None)
}

impl Offer {
    /// The control's label, naming what happens on this platform.
    pub(crate) fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            "System preview"
        } else {
            "Open in default app"
        }
    }

    /// The label of a control that names its side ("Before", "After").
    pub(crate) fn side_label(self, side: &str) -> String {
        if cfg!(target_os = "macos") {
            format!("{side} system preview")
        } else {
            format!("Open {side} in default app")
        }
    }
}

#[cfg(target_os = "linux")]
impl Offer {
    /// What the click hands over, so the tooltip names the type the
    /// application receives rather than the repository's file name.
    pub(crate) fn tooltip(self) -> String {
        format!(
            "Open a read-only copy of these captured bytes as {} in the desktop's default app",
            self.kind.description()
        )
    }

    /// The accessible name of a control on the `side` ("Before", "After",
    /// "Source") side, which a visible "Open in default app" leaves out.
    pub(crate) fn accessible_name(self, side: &str) -> String {
        format!(
            "{side}: open a read-only copy as {} in the default app",
            self.kind.description()
        )
    }

    /// The sentence that tells a reader where the complete animation is.
    pub(crate) fn complete_animation(self) -> &'static str {
        "Open captured bytes in the default app for the complete animation."
    }
}

#[cfg(not(target_os = "linux"))]
impl Offer {
    pub(crate) fn tooltip(self) -> String {
        "Open an isolated read-only copy of these captured bytes in system Quick Look".into()
    }

    pub(crate) fn accessible_name(self, side: &str) -> String {
        format!("{side}: open captured bytes in system preview")
    }

    pub(crate) fn complete_animation(self) -> &'static str {
        "Open captured bytes in system preview for the complete animation."
    }
}

/// Removes copies that sessions which have ended left behind, off the UI
/// thread, and this session's own copies when the app quits.
#[cfg(target_os = "linux")]
pub(crate) fn init(cx: &mut gpui_kit::App) {
    cx.background_executor()
        .spawn(async {
            if let Some(store) = store::shared() {
                let _ = store.remove_stale();
            }
        })
        .detach();
    cx.on_app_quit(|cx| {
        cx.background_executor().spawn(async {
            if let Some(store) = store::shared() {
                store.close();
            }
        })
    })
    .detach();
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn init(_cx: &mut gpui_kit::App) {}

#[cfg(target_os = "linux")]
impl crate::GitTurtle {
    /// Writes the private copy on the serialized background writer, hands it
    /// to the desktop off the UI thread, and reports the outcome in the
    /// operation notice. Nothing here retries or opens anything else.
    pub(crate) fn open_in_default_app(
        &mut self,
        bytes: std::sync::Arc<[u8]>,
        offer: Offer,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let kind = offer.kind;
        let slot = match crate::rich_preview::ExternalPreviewSlot::reserve() {
            Ok(slot) => slot,
            Err(error) => {
                self.operation_notice = Some(format!("{error:#}"));
                cx.notify();
                return;
            }
        };
        let copy = self.preferences_writer.submit_read(move || {
            store::shared()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "The user cache folder is unavailable: neither XDG_CACHE_HOME nor HOME names an absolute path."
                    )
                })?
                .write(&bytes, kind)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = match copy.await {
                Ok(Ok(copy)) => {
                    let executor = cx.background_executor().clone();
                    executor
                        .clone()
                        .spawn(launch::open(copy, kind, slot, executor))
                        .await
                }
                Ok(Err(error)) => Err(format!("{error:#}")),
                Err(_) => Err("The copy stopped before it was written.".into()),
            };
            let _ = this.update_in(cx, |this, _, cx| {
                this.operation_notice = Some(outcome(result));
                cx.notify();
            });
        })
        .detach();
    }
}

/// The operation notice for one click: what opened, or why nothing did.
#[cfg(target_os = "linux")]
fn outcome(result: Result<(), String>) -> String {
    match result {
        Ok(()) => "Opened a read-only copy of the captured bytes in the default app.".into(),
        Err(reason) => {
            let reason = reason.trim_end_matches('.');
            format!("Could not open in the default app. {reason}.")
        }
    }
}
