//! Explicit LFS preview downloads with captured pointer/source review.
use crate::*;
use gitturtle_core::{LfsDownloadPlan, LfsDownloadTarget, WriteCommand};
use gpui_kit::{
    component::{WindowExt, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

fn label(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

impl GitTurtle {
    pub(super) fn render_lfs_download_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(file) = self.selected_file.and_then(|index| self.files.get(index)) else {
            return div().into_any_element();
        };
        let pointers = match self.content.as_deref() {
            Some(Content::Images { old, new }) => {
                [old.lfs_pointer.clone(), new.lfs_pointer.clone()]
            }
            Some(Content::Text { old, new, .. }) => [old, new].map(|bytes| {
                gitturtle_preview::detect_lfs_pointer(bytes.as_bytes())
                    .map(|_| bytes.as_bytes().to_vec())
            }),
            _ => [None, None],
        };
        let targets = pointers
            .into_iter()
            .enumerate()
            .filter_map(|(index, pointer)| {
                let pointer = pointer?;
                let path = if index == 0 {
                    file.old_path.clone()
                } else {
                    file.new_path.clone()
                }?;
                let blob_oid = if index == 0 {
                    file.old_oid.clone()
                } else {
                    file.new_oid.clone()
                };
                Some((
                    index,
                    LfsDownloadTarget {
                        path,
                        blob_oid,
                        pointer,
                    },
                ))
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return div().into_any_element();
        }
        div()
            .px_3()
            .py_2()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                label(
                    "lfs-preview-download-explanation",
                    "LFS preview objects require an explicit download.",
                )
                .text_size(appearance::ui_text(12.)),
            )
            .children(targets.into_iter().map(|(side, target)| {
                button(
                    ("download-lfs-side", side),
                    if side == 0 {
                        "Download Before LFS…"
                    } else {
                        "Download After LFS…"
                    },
                    "download",
                    false,
                )
                .disabled(self.operation_busy.is_some())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_lfs_download(target.clone(), window, cx)
                }))
            }))
            .into_any_element()
    }

    fn open_lfs_download(
        &mut self,
        target: LfsDownloadTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let remotes = self
            .remotes
            .iter()
            .map(|remote| remote.name.clone())
            .collect();
        let form =
            cx.new(|cx| LfsDownloadForm::new(owner, repo, target, remote, remotes, window, cx));
        form.update(cx, |this, cx| this.prepare(window, cx));
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(label("lfs-download-title", "Download an LFS preview"))
                .width(px(650.))
                .child(form.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Review download")
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.review(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
        });
    }
}

struct LfsDownloadForm {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    target: LfsDownloadTarget,
    remote: Entity<InputState>,
    remotes: Vec<String>,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    cancellation: gitturtle_core::HistoryCancellation,
    plan: Option<LfsDownloadPlan>,
    pending: bool,
    error: Option<String>,
    _subscription: Subscription,
}
impl Drop for LfsDownloadForm {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl LfsDownloadForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        target: LfsDownloadTarget,
        remote: String,
        remotes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remote = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(remote)
                .placeholder("Configured remote name")
        });
        let subscription = cx.subscribe_in(
            &remote,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.cancel();
                    this.plan = None;
                }
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.prepare(window, cx);
                }
                cx.notify();
            },
        );
        Self {
            owner,
            repo,
            target,
            remote,
            remotes,
            reader: operations::SerialExecutor::new("lfs-preview-source"),
            task: None,
            cancellation: Default::default(),
            plan: None,
            pending: false,
            error: None,
            _subscription: subscription,
        }
    }
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.pending = false;
    }
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel();
        self.plan = None;
        self.error = None;
        let remote = self.remote.read(cx).value().trim().to_owned();
        if remote.is_empty() {
            self.error = Some("Choose a configured remote for this LFS object. Add a remote from the branch menu if this repository has none.".into());
            cx.notify();
            return;
        }
        self.pending = true;
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let repo = self.repo.clone();
        let target = self.target.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || {
                repo.lfs_download_plan(&target, &remote)
            })
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "LFS source inspection was interrupted. Read the source again."
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                this.pending = false;
                match result {
                    Ok(plan) => this.plan = Some(plan),
                    Err(error) => this.error = Some(format!("{error:#}")),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(plan) = self.plan.clone() else {
            self.prepare(window, cx);
            return;
        };
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_deref() != Some(this_path(&self.repo)) || owner.operation_busy.is_some() { self.error = Some("The repository changed or another operation started. Reopen this download after it finishes.".into()); cx.notify(); return; }
            window.close_dialog(cx);
            owner.confirm_git_write("Download selected LFS object".into(), format!("File: {}\nObject: sha256:{}\nSize: {} bytes\nRemote: {}\n{}\n\nThis explicit transfer fetches only this object's pointer. Git LFS uses your configured credentials and SSH host verification. The download can be cancelled from the operation bar.\n\nGitTurtle verifies the object's SHA-256 and size before previewing it. Working files and the index remain unchanged; no checkout, smudge, prune, or push is performed.", plan.target.path.display(), plan.oid, plan.size, plan.remote, plan.source), "Download object", WriteCommand::DownloadLfs(Arc::new(plan)), window, cx);
        });
    }
}
fn this_path(repo: &GitRepository) -> &std::path::Path {
    repo.path()
}

impl Render for LfsDownloadForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pointer = gitturtle_preview::detect_lfs_pointer(&self.target.pointer);
        div().id("lfs-download-review-content").flex().flex_col().gap_3().max_h(px(450.)).overflow_y_scroll()
            .child(label("lfs-download-file", format!("File: {}", self.target.path.display())).text_size(appearance::ui_text(13.)))
            .when_some(pointer.as_ref(), |element, pointer| element.child(label("lfs-download-object", format!("sha256:{}\n{} bytes", pointer.oid, pointer.size)).text_size(appearance::ui_text(12.))))
            .child(label("lfs-download-source-label", "Choose the configured source").text_size(appearance::ui_text(12.)))
            .child(Input::new(&self.remote).aria_label("Git LFS download remote"))
            .child(div().flex().flex_wrap().gap_2().children(self.remotes.iter().take(12).enumerate().map(|(index, name)| { let name = name.clone(); button(("lfs-source-remote", index), name.clone(), "", false).disabled(self.pending).on_click(cx.listener(move |this, _, window, cx| { this.remote.update(cx, |input, cx| input.set_value(name.clone(), window, cx)); this.prepare(window, cx); })) })))
            .child(button("read-lfs-source", "Read configured source", "refresh-cw", false).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.prepare(window, cx))))
            .when(self.pending, |element| element.child(label("lfs-source-loading", "Resolving the selected pointer, Git LFS tooling, and configured source…").text_size(appearance::ui_text(12.))))
            .when_some(self.plan.as_ref(), |element, plan| element.child(label("lfs-source-resolved", plan.source.clone()).text_size(appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| label("lfs-download-error", error.clone()).text_size(appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .child(label("lfs-download-limit", "Normal browsing stays passive. This action is limited to one object up to 32 MiB; decoder and memory limits still apply. No credentials are stored in this form.").text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)))
    }
}
