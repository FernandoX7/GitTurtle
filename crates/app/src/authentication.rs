//! Transient authentication UI. The helper/agent owns durable credentials;
//! no response enters preferences, recents, operation notices or diagnostics.
use crate::*;
use gitturtle_core::{AuthenticationPrompt, OperationControl};
use gpui_kit::component::{WindowExt, dialog::DialogButtonProps, input::InputContentType};
use std::time::Duration;

#[derive(Default)]
pub(super) struct State {
    control: Option<OperationControl>,
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

    pub(super) fn finish_operation_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.authentication.control = None;
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

    pub(super) fn render_operation_cancel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(control) = &self.authentication.control else {
            return div().into_any_element();
        };
        Button::new("cancel-git-operation")
            .secondary().small()
            .label(if control.is_cancelled() { "Cancelling…" } else { "Cancel operation" })
            .disabled(control.is_cancelled())
            .tooltip("Stop Git and its child processes. Local or remote changes may already have applied; inspect before retrying.")
            .accessibility_label("Cancel the running Git operation")
            .on_click(cx.listener(|this, _, window, cx| this.cancel_operation(window, cx)))
            .into_any_element()
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
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let submit_control = submit_control.clone();
            let input = input.clone();
            let submit_input = input.clone();
            let submitted = Arc::clone(&submitted);
            let closed = Arc::clone(&submitted);
            let close_control = close_control.clone();
            let owner = owner.clone();
            let body = div().flex().flex_col().gap_3()
                .child(div().text_size(px(13.)).child(prompt.message.clone()))
                .child(div().text_size(px(12.)).child(if confirmation {
                    "Verify this host and fingerprint through a trusted source before continuing. Accepting lets SSH update its configured known-hosts file."
                } else {
                    "Requested by the Git operation you started. Your configured credential helper may save this response, including in macOS Keychain. GitTurtle does not save it."
                }));
            let body = if confirmation { body } else {
                let field = Input::new(&input).aria_label(format!("{}: {}", if prompt.secret { "Git authentication secret" } else { "Git username" }, prompt.message));
                let field = if prompt.secret { field.content_type(InputContentType::Password) } else { field };
                body.child(field)
            };
            dialog.title(if confirmation { "Verify SSH host" } else { "Git authentication" })
                .width(px(540.)).child(body)
                .button_props(DialogButtonProps::default().ok_text(if confirmation { "Trust verified host" } else { "Continue" }).cancel_text("Cancel operation").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let response = if confirmation { "yes".to_owned() } else { submit_input.read(cx).unmask_value().to_string() };
                    if !submit_control.answer(prompt_id, Some(response)) { return false; }
                    submitted.store(true, std::sync::atomic::Ordering::Release);
                    submit_input.update(cx, |input, cx| input.set_value("", window, cx));
                    true
                })
                .on_close(move |_, window, cx| {
                    if !closed.load(std::sync::atomic::Ordering::Acquire) { close_control.cancel(); }
                    let _ = owner.update(cx, |this, cx| {
                        this.authentication.prompt_open = false;
                        this.authentication.prompt_id = None;
                        if let Some(focus) = this.authentication.return_focus.take() { focus.focus(window, cx); }
                        cx.notify();
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
