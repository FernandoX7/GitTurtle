//! Transient authentication UI. The helper/agent owns durable credentials;
//! no response enters preferences, recents, operation notices or diagnostics.
use crate::*;
use gitturtle_core::{AuthenticationPrompt, HistoryCancellation, OperationControl};
use gpui_kit::component::{WindowExt, dialog::DialogButtonProps, input::InputContentType};
use std::time::Duration;

const HOST_GUIDANCE: &str = "Verify this host and fingerprint through a trusted source before continuing. Accepting lets SSH update its configured known-hosts file.";
/// Keychain exists only on macOS; elsewhere the configured helper's store is
/// left unnamed.
const CREDENTIAL_GUIDANCE: &str = if cfg!(target_os = "macos") {
    "Requested by the Git operation you started. Your configured credential helper may save this response, including in macOS Keychain. GitTurtle does not save it."
} else {
    "Requested by the Git operation you started. Your configured credential helper may save this response. GitTurtle does not save it."
};

/// Cancel's labels while the write runs and once it is cancelled. The header
/// keeps room for the wider (`views::OperationStatus`), so Cancel changing
/// its label moves nothing.
pub(crate) const CANCEL_LABELS: [&str; 2] = ["Cancel operation", "Cancelling…"];

/// Cancel's accessible name in either form, labelled or its icon.
pub(crate) const CANCEL_NAME: &str = "Cancel the running Git operation";

const CANCEL_TOOLTIP: &str = "Stop Git and its child processes. Local or remote changes may already have applied; inspect before retrying.";

/// One string supplies the prompt's visible guidance and the static text it
/// is read as.
fn guidance(confirmation: bool) -> native_accessibility::StaticText<Stateful<Div>> {
    let text = if confirmation {
        HOST_GUIDANCE
    } else {
        CREDENTIAL_GUIDANCE
    };
    native_accessibility::text("git-authentication-guidance", text)
        .text_size(crate::appearance::ui_text(12.))
}

#[derive(Default)]
pub(super) struct State {
    control: Option<OperationControl>,
    /// A local read the running operation does through its own
    /// cancellation, which its Cancel stops too
    /// ([`GitTurtle::cancel_with_operation`]).
    local: Option<HistoryCancellation>,
    task: Option<Task<()>>,
    prompt_open: bool,
    prompt_id: Option<u64>,
    return_focus: Option<FocusHandle>,
}

impl GitTurtle {
    pub(super) fn begin_operation_control(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> OperationControl {
        let control = OperationControl::default();
        self.authentication.control = Some(control.clone());
        self.authentication.local = None;
        self.authentication.task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut previous_progress = None;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let active = this
                    .update_in(cx, |this, window, cx| {
                        let Some(control) = this.authentication.control.clone() else {
                            return false;
                        };
                        if let Some(prompt) = control.take_prompt() {
                            this.show_authentication_prompt(control.clone(), prompt, window, cx);
                            cx.notify();
                        }
                        let progress = control.progress();
                        if progress != previous_progress {
                            previous_progress = progress;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        }));
        control
    }

    /// Lets the running operation's Cancel stop `local` too: a read the
    /// operation does through its own cancellation, as a PR's Changes
    /// compares its revisions, which the control alone never reached, so
    /// the operation bar's Cancel left the comparison running.
    pub(super) fn cancel_with_operation(&mut self, local: HistoryCancellation) {
        if self.authentication.control.is_some() {
            self.authentication.local = Some(local);
        }
    }

    pub(super) fn finish_operation_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.authentication.control = None;
        self.authentication.local = None;
        self.authentication.task = None;
        if self.authentication.prompt_open {
            window.close_dialog(cx);
            self.authentication.prompt_open = false;
            self.authentication.prompt_id = None;
            if let Some(focus) = self.authentication.return_focus.take() {
                focus.focus(window, cx);
            }
        }
    }

    pub(super) fn cancel_operation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(control) = &self.authentication.control {
            control.cancel();
        }
        if let Some(local) = &self.authentication.local {
            local.cancel();
        }
        cx.notify();
    }

    fn finish_authentication_prompt(
        &mut self,
        prompt_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.authentication.prompt_id != Some(prompt_id) {
            return;
        }
        self.authentication.prompt_open = false;
        self.authentication.prompt_id = None;
        if let Some(focus) = self.authentication.return_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn operation_progress(&self) -> Option<String> {
        let control = self.authentication.control.as_ref()?;
        if control.is_cancelled() {
            Some("Cancelling… Inspect the result before retrying.".into())
        } else if self.authentication.prompt_open {
            Some("Waiting for your authentication response…".into())
        } else {
            control.progress()
        }
    }

    /// Cancel for the running write ([`Self::operation_cancel`]), or nothing
    /// where no write can be cancelled.
    pub(super) fn render_operation_cancel(&self, icon: bool, cx: &mut Context<Self>) -> AnyElement {
        self.operation_cancel(icon, cx)
            .map_or_else(|| div().into_any_element(), IntoElement::into_any_element)
    }

    /// Cancel for the running write: labelled, or where `icon` the shared
    /// 28 px control with its icon, its label leading its tooltip. Either
    /// keeps one accessible name ([`CANCEL_NAME`]).
    pub(super) fn operation_cancel(&self, icon: bool, cx: &mut Context<Self>) -> Option<Button> {
        let control = self.authentication.control.as_ref()?;
        let label = CANCEL_LABELS[usize::from(control.is_cancelled())];
        let cancel = if icon {
            let tooltip = format!("{label} · {CANCEL_TOOLTIP}");
            crate::views::note_control_tooltip(&"cancel-git-operation".into(), &tooltip);
            crate::views::compact_control_name(
                crate::button("cancel-git-operation", "", "close", false),
                "cancel-git-operation",
                CANCEL_NAME,
            )
            .tooltip(tooltip)
        } else {
            Button::new("cancel-git-operation")
                .secondary()
                .small()
                .label(label)
                .tooltip(CANCEL_TOOLTIP)
                .accessibility_label(CANCEL_NAME)
        };
        Some(
            cancel
                .debug_selector(|| "cancel-git-operation".into())
                .disabled(control.is_cancelled())
                .on_click(cx.listener(|this, _, window, cx| this.cancel_operation(window, cx))),
        )
    }

    fn show_authentication_prompt(
        &mut self,
        control: OperationControl,
        prompt: AuthenticationPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.authentication.return_focus = window.focused(cx);
        self.authentication.prompt_open = true;
        self.authentication.prompt_id = Some(prompt.id);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(prompt.secret)
                .placeholder(if prompt.secret {
                    "Password, token, or key passphrase"
                } else {
                    "Username"
                })
        });
        let focus_input = input.clone();
        let owner = cx.entity().downgrade();
        let submit_control = control.clone();
        let close_control = control;
        let submitted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let prompt_id = prompt.id;
        let confirmation = prompt.confirmation;
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let submit_control = submit_control.clone();
            let input = input.clone();
            let submit_input = input.clone();
            let cancel_input = input.clone();
            let close_input = input.clone();
            let submitted = Arc::clone(&submitted);
            let closed = Arc::clone(&submitted);
            let cancel_control = close_control.clone();
            let close_control = close_control.clone();
            let submit_owner = owner.clone();
            let cancel_owner = owner.clone();
            let owner = owner.clone();
            let body = div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    native_accessibility::text(
                        "git-authentication-message",
                        prompt.message.clone(),
                    )
                    .text_size(crate::appearance::ui_text(13.)),
                )
                .child(guidance(confirmation));
            let body = if confirmation {
                body
            } else {
                let field = Input::new(&input).aria_label(format!(
                    "{}: {}",
                    if prompt.secret {
                        "Git authentication secret"
                    } else {
                        "Git username"
                    },
                    prompt.message
                ));
                let field = if prompt.secret {
                    field.content_type(InputContentType::Password)
                } else {
                    field
                };
                body.child(field)
            };
            // Trusting a host interrupts with a warning; a credential is a form.
            dialog
                .kind(if confirmation {
                    DialogKind::Alert
                } else {
                    DialogKind::Dialog
                })
                .title(if confirmation {
                    "Verify SSH host"
                } else {
                    "Git authentication"
                })
                .width(appearance::dialog_width(px(540.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(body)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(if confirmation {
                            "Trust verified host"
                        } else {
                            "Continue"
                        })
                        .cancel_text("Cancel operation")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let response = if confirmation {
                        "yes".to_owned()
                    } else {
                        submit_input.read(cx).unmask_value().to_string()
                    };
                    if !submit_control.answer(prompt_id, Some(response)) {
                        return false;
                    }
                    submitted.store(true, std::sync::atomic::Ordering::Release);
                    submit_input.update(cx, |input, cx| input.set_value("", window, cx));
                    let _ = submit_owner.update(cx, |this, cx| {
                        this.finish_authentication_prompt(prompt_id, window, cx);
                    });
                    true
                })
                // The pinned AlertDialog facade replaces its base button props
                // while rendering, so its on_close callback can be lost. The
                // explicit button callbacks also cover Enter and Escape.
                .on_cancel(move |_, window, cx| {
                    cancel_control.cancel();
                    cancel_input.update(cx, |input, cx| input.set_value("", window, cx));
                    let _ = cancel_owner.update(cx, |this, cx| {
                        this.finish_authentication_prompt(prompt_id, window, cx);
                    });
                    true
                })
                .on_close(move |_, window, cx| {
                    if !closed.load(std::sync::atomic::Ordering::Acquire) {
                        close_control.cancel();
                    }
                    close_input.update(cx, |input, cx| input.set_value("", window, cx));
                    let _ = owner.update(cx, |this, cx| {
                        this.finish_authentication_prompt(prompt_id, window, cx);
                    });
                })
        });
        if !confirmation {
            // The modal focus trap is installed during paint. Focusing before
            // that frame can leave focus on its container instead of the input.
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.authentication.prompt_open
                        && this.authentication.prompt_id == Some(prompt_id)
                    {
                        focus_input.read(cx).focus_handle(cx).focus(window, cx);
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    /// The macOS wording, unchanged since the prompt first named Keychain.
    const MACOS_CREDENTIAL_GUIDANCE: &str = "Requested by the Git operation you started. Your configured credential helper may save this response, including in macOS Keychain. GitTurtle does not save it.";
    /// The owner's wording for every other platform.
    const OTHER_CREDENTIAL_GUIDANCE: &str = "Requested by the Git operation you started. Your configured credential helper may save this response. GitTurtle does not save it.";

    #[gpui::test]
    async fn pending_credential_prompt_names_keychain_only_on_macos(cx: &mut TestAppContext) {
        // GitTurtle::new loads profiles on the real preferences executor; the
        // barrier below drains it before teardown.
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        let prompt = AuthenticationPrompt {
            id: 0,
            message: "Password for 'https://git.example.invalid':".into(),
            secret: true,
            confirmation: false,
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_authentication_prompt(OperationControl::default(), prompt, window, cx)
            });
        });
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.executor().run_until_parked();
        cx.update(|window, cx| assert!(window.has_active_dialog(cx)));
        assert!(app.read_with(cx, |app, _| app.authentication.prompt_open));
        assert!(cx.debug_bounds("git-authentication-guidance").is_some());

        // The test window never activates accessibility and painted text has no
        // reader, so read the element the pending dialog renders from its builder.
        let element = guidance(false);
        let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
        element.write_a11y_info(&mut node);
        // A label is named by its value.
        let (text, name) = (CREDENTIAL_GUIDANCE, node.value().unwrap_or_default());
        assert_eq!(name, text);
        if cfg!(target_os = "macos") {
            assert_eq!(text, MACOS_CREDENTIAL_GUIDANCE);
        } else {
            assert_eq!(text, OTHER_CREDENTIAL_GUIDANCE);
            assert_eq!(name, OTHER_CREDENTIAL_GUIDANCE);
            assert!(!text.contains("Keychain"), "visible guidance: {text}");
            assert!(!name.contains("Keychain"), "accessible name: {name}");
        }

        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.finish_operation_control(window, cx));
            assert!(!window.has_active_dialog(cx));
        });
        assert!(!app.read_with(cx, |app, _| app.authentication.prompt_open));
        let preferences_done =
            app.read_with(cx, |app, _| app.preferences_writer.submit_read(|| Ok(())));
        preferences_done.await.unwrap().unwrap();
        cx.executor().run_until_parked();
    }
}
