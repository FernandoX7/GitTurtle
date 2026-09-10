//! Native GitHub collaboration. Repository/PR actions capture their destination;
//! network work exists only behind an explicit button or submit confirmation.
use crate::github::{
    self, Action, Client, ComparisonTarget, NewPull, PullRequest, PullStatus, Repository,
    ReviewDraft, ReviewEvent,
    drafts::{self, Draft},
    transport::{CredentialStore, GhTransport, SecureStore},
};
use crate::*;
use gitturtle_core::{HistoryCancellation, OperationControl};
use gpui_kit::{
    component::{WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};

fn label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
        .text_size(crate::appearance::ui_text(12.))
}
impl GitTurtle {
    pub(super) fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let branch = self.remote_branch.read(cx).value().to_string();
        let form = cx.new(|cx| Panel::new(owner, repo, branch, window, cx));
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                .title(label("github-title", "GitHub pull requests"))
                .width(px(860.))
                .child(form.clone())
                .footer(DialogFooter::new().child(
                    button("github-close", "Back to repository", "", false).on_click(
                        move |_, window, cx| close.update(cx, |this, cx| this.close(window, cx)),
                    ),
                ))
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |this, cx| this.close(window, cx));
                    false
                })
        });
    }
}
struct Confirmation {
    action: Action,
    account: String,
}
struct Panel {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    destination: Entity<InputState>,
    head: Entity<InputState>,
    base: Entity<InputState>,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    rows: Vec<PullRequest>,
    page: u32,
    has_next: bool,
    pull: Option<PullRequest>,
    status: Option<PullStatus>,
    description: Option<Entity<EditorState>>,
    create: bool,
    draft: bool,
    event: ReviewEvent,
    discussion: bool,
    account: Option<String>,
    notice: Option<String>,
    error: Option<String>,
    pending: bool,
    writing: bool,
    closed: bool,
    confirm: Option<Confirmation>,
    control: Option<OperationControl>,
    task: Option<Task<()>>,
    saved: Vec<Draft>,
    save_status: String,
    saver: commit_drafts::CoalescingSaver<String, Draft>,
    subscriptions: Vec<Subscription>,
    templates: Vec<github::Template>,
    attempts: Vec<drafts::Attempt>,
    local_control: Option<HistoryCancellation>,
}
impl Panel {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this=Self {
            owner,repo,destination:cx.new(|cx|InputState::new(window,cx).placeholder("owner/repository")),
            head:cx.new(|cx|InputState::new(window,cx).default_value(branch).placeholder("Published head branch (owner:branch for a fork)")),
            base:cx.new(|cx|InputState::new(window,cx).default_value("main")),title:cx.new(|cx|InputState::new(window,cx).placeholder("Pull request title")),
            body:cx.new(|cx|TextareaState::new(window,cx).rows(5).placeholder("Description or review text — saved locally")),
            rows:vec![],page:1,has_next:false,pull:None,status:None,description:None,create:false,draft:true,event:ReviewEvent::Comment,discussion:true,
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),error:None,pending:false,writing:false,closed:false,
            confirm:None,control:None,task:None,saved:vec![],save_status:String::new(),saver:Default::default(),subscriptions:vec![],templates:vec![],attempts:vec![],local_control:None,
        };
        for input in [this.head.clone(), this.base.clone(), this.title.clone()] {
            this.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.confirm = None;
                        this.save_draft(window, cx);
                    }
                },
            ));
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.body,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.save_draft(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.destination,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.rows.clear();
                    this.pull = None;
                    this.status = None;
                    this.description = None;
                    this.confirm = None;
                    this.page = 1;
                    this.has_next = false;
                    cx.notify();
                }
            },
        ));
        this.load_local(window, cx);
        this
    }
    fn load_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit_read(move || {
                Ok((repo.remote_configs()?, drafts::load(), drafts::attempts()))
            })
        });
        let Ok(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.closed {
                    return;
                }
                match result {
                    Ok(Ok((remotes, saved, attempts))) => {
                        if this.destination.read(cx).value().is_empty()
                            && let Some(destination) = remotes
                                .iter()
                                .flat_map(|remote| remote.urls.iter())
                                .find_map(|url| Repository::parse(url).ok())
                        {
                            this.destination.update(cx, |input, cx| {
                                input.set_value(destination.label(), window, cx)
                            });
                        }
                        match attempts {
                            Ok(attempts) => this.attempts = attempts,
                            Err(error) => this.error = Some(format!("{error:#}")),
                        }
                        match saved {
                            Ok(saved) => this.saved = saved,
                            Err(error) => this.error = Some(format!("{error:#}")),
                        }
                    }
                    _ => {
                        this.error =
                            Some("Could not read repository remotes and saved drafts.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn destination(&self, cx: &App) -> anyhow::Result<Repository> {
        Repository::parse(&self.destination.read(cx).value())
    }
    fn current_draft(&self, cx: &App) -> Option<Draft> {
        if self.create {
            Some(Draft::Pull(NewPull {
                repository: self.destination(cx).ok()?,
                title: self.title.read(cx).value().to_string(),
                body: self.body.read(cx).value().to_string(),
                head: self.head.read(cx).value().to_string(),
                base: self.base.read(cx).value().to_string(),
                draft: self.draft,
                expected_head: None,
                expected_base: None,
            }))
        } else {
            Some(Draft::Review(ReviewDraft {
                pull: self.pull.as_ref()?.capture(self.destination(cx).ok()?),
                body: self.body.read(cx).value().to_string(),
                event: self.event,
                comments: vec![],
            }))
        }
    }
    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.current_draft(cx) else {
            return;
        };
        if draft.text().is_empty() && matches!(&draft,Draft::Pull(p) if p.title.is_empty()) {
            return;
        }
        let key = draft.key();
        if let Some(old) = self.saved.iter_mut().find(|d| d.key() == key) {
            *old = draft.clone();
        } else {
            self.saved.push(draft.clone());
        }
        let response = self
            .owner
            .update(cx, |owner, _| {
                self.saver
                    .queue_with(&owner.preferences_writer, key, draft, drafts::save_batch)
            })
            .ok()
            .flatten();
        self.save_status = "Saving local draft…".into();
        if let Some(response) = response {
            cx.spawn_in(window, async move |this, cx| {
                let result = response.await;
                let _ = this.update_in(cx, |this, _, cx| {
                    match result {
                        Ok(Ok(())) => this.save_status = "Draft saved locally".into(),
                        _ => {
                            this.save_status =
                                "Draft save failed — keep this window open or copy the text".into();
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
    fn run<T: Send + 'static>(
        &mut self,
        writing: bool,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.closed {
            return;
        }
        let control = OperationControl::default();
        let worker_control = control.clone();
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit(move || operation(worker_control))
        });
        let Ok(response) = response else {
            self.error = Some("The repository window is unavailable.".into());
            return;
        };
        self.control = Some(control);
        self.pending = true;
        self.writing = writing;
        self.error = None;
        self.task=Some(cx.spawn_in(window,async move|this,cx|{
            let result=response.await.unwrap_or_else(|_|Err(anyhow::anyhow!("GitHub operation was interrupted. Its outcome may be uncertain; check the destination before submitting again.")));
            let _=this.update_in(cx,|this,window,cx|{this.pending=false;this.writing=false;this.control=None;if !this.closed {receive(result,this,window,cx);}cx.notify();});
        }));
        cx.notify();
    }
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            false,
            |control| GhTransport::connect_existing(&control),
            |result, this, _, _| match result {
                Ok(login) => {
                    this.notice = Some(format!(
                        "Connected as {login}; authorization is stored in macOS Keychain."
                    ));
                    this.account = Some(login);
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn refresh(&mut self, page: u32, window: &mut Window, cx: &mut Context<Self>) {
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        self.run(false,move|control|{let transport=GhTransport::stored()?;let login=transport.login().to_owned();Ok((Client(transport).list(&repository,page,&control)?,login))},|result,this,_,_|match result {Ok((page,login))=>{this.rows=page.items;this.page=page.page;this.has_next=page.has_next;this.account=Some(login);this.notice=Some("PR list refreshed. Selecting a PR explicitly loads its current review and check status.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn inspect(&mut self, pull: PullRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        self.run(false,move|control|{let mut client=Client(GhTransport::stored()?);let pull=client.pull(&repository,pull.number,&control)?;let status=client.status(&pull.capture(repository),&control);Ok((pull,status))},|result,this,window,cx|match result {Ok((pull,status))=>{
            this.create=false;this.confirm=None;this.description=pull.body.as_ref().map(|body|text::editor(body,"markdown",None,window,cx));
            match status {Ok(status)=>this.status=Some(status),Err(error)=>{this.status=None;this.error=Some(format!("PR loaded; some review/check status is unavailable: {error:#}"));}}
            let captured=pull.capture(this.destination(cx).expect("captured repository"));this.pull=Some(pull);
            let draft=this.saved.iter().find_map(|draft|match draft {Draft::Review(review) if review.pull==captured=>Some(review.clone()),_=>None});
            this.body.update(cx,|input,cx|input.set_value(draft.as_ref().map(|d|d.body.clone()).unwrap_or_default(),window,cx));
            if let Some(draft)=draft {this.event=draft.event;}
            let older=this.saved.iter().any(|draft|matches!(draft,Draft::Review(review) if review.pull.repository==captured.repository && review.pull.number==captured.number && review.pull!=captured));
            this.notice=Some(if older {"An older-head review draft is retained below. Copy it for recovery; review its text and positions against this new head before submission."}else{"Head and base are captured. Reviews recheck both before submission; changes open only locally available objects."}.into());
        },Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        self.create = true;
        self.pull = None;
        self.status = None;
        self.description = None;
        self.confirm = None;
        self.body
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice=Some("Use a branch already published to GitHub. Creating a PR never pushes a branch. Choose Draft or Ready, then review the destination.".into());
        cx.notify();
    }
    fn load_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let revision = self.base.read(cx).value().to_string();
        self.run(false,move|_|github::local_templates(&repo,&revision),|result,this,_,_|match result {Ok(templates)=>{this.templates=templates;this.notice=Some("Templates came from the locally available base revision. Choose one to append its exact text; repository HTML is never executed.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn review_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.current_draft(cx) else {
            self.error = Some("Choose a pull request or create a new one first.".into());
            return;
        };
        if let Draft::Pull(pull) = draft {
            self.run(
                false,
                move |control| {
                    let transport = GhTransport::stored()?;
                    let account = transport.login().to_owned();
                    Ok((Client(transport).prepare_create(pull, &control)?, account))
                },
                |result, this, _, _| match result {
                    Ok((pull, account)) => {
                        this.account = Some(account.clone());
                        this.confirm = Some(Confirmation {
                            action: Action::Create(pull),
                            account,
                        });
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                },
                window,
                cx,
            );
            return;
        }
        let Some(account) = self.account.clone() else {
            self.error = Some("Connect and refresh the GitHub account before submission.".into());
            return;
        };
        self.confirm = Some(Confirmation {
            account,
            action: match draft {
                Draft::Pull(_) => unreachable!(),
                Draft::Review(p) if self.discussion => Action::Comment {
                    pull: p.pull,
                    body: p.body,
                },
                Draft::Review(p) => Action::Review(p),
            },
        });
        cx.notify();
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirmation) = self.confirm.take() else {
            return;
        };
        self.save_draft(window, cx);
        self.run(true,move|control|{let transport=GhTransport::stored()?;anyhow::ensure!(transport.login()==confirmation.account,"The connected GitHub account changed. Review the destination again before submitting.");let destination=format!("{} · account {}",confirmation.action.destination(),confirmation.account);drafts::record_attempt(destination.clone(),"Submission started; if no final outcome appears, inspect GitHub before sending again.".into())?;let result=Client(transport).execute(confirmation.action,&control);let outcome=match &result{Ok(notice)=>notice.clone(),Err(error)=>format!("{error:#}")};if let Err(error)=drafts::record_attempt(destination,outcome.chars().take(4000).collect()){return Err(anyhow::anyhow!("The GitHub action finished but saving its outcome failed: {error}. Check GitHub before resubmitting."));}result},|result,this,_,_|match result {Ok(notice)=>{this.notice=Some(format!("{notice}. The local draft remains available; submitting it again creates another outbound action."));},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending && self.writing {
            self.error=Some("A GitHub action is in progress for this captured destination. Wait for the result, or choose Cancel and inspect GitHub before resubmitting.".into());
            cx.notify();
            return;
        }
        if let Some(control) = &self.control {
            control.cancel();
        }
        if let Some(control) = &self.local_control {
            control.cancel();
        }
        self.save_draft(window, cx);
        self.closed = true;
        window.close_dialog(cx);
        if let Some(owner) = self.owner.upgrade() {
            let focus = owner.read(cx).app_focus.clone();
            focus.focus(window, cx);
        }
    }
    fn changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self
            .pull
            .as_ref()
            .and_then(|pull| self.destination(cx).ok().map(|repo| pull.capture(repo)))
        else {
            return;
        };
        let target = ComparisonTarget {
            worktree: self.repo.path().to_owned(),
            pull,
        };
        let repo = self.repo.clone();
        let cancellation = HistoryCancellation::default();
        self.local_control = Some(cancellation.clone());
        self.run(false, move |_| {
            let comparison = repo.compare_revisions(&target.pull.base.sha, &target.pull.head.sha, gitturtle_core::ComparisonMode::SinceBranching, &cancellation)?;
            Ok((target.worktree, comparison))
        }, |result, this, window, cx| match result {
            Ok((path, comparison)) => {
                this.close(window, cx);
                let _ = this.owner.update(cx, |owner, cx| {
                    if owner.path.as_ref() == Some(&path) && owner.page == AppPage::Repository { owner.show_revision_comparison(comparison, window, cx); }
                });
            },
            Err(error) => this.error = Some(format!("PR changes need their captured head, base, and common ancestor locally. Use an explicit Fetch if needed, then reopen the PR. {error:#}")),
        }, window, cx);
    }
    fn discard_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let key = draft.key();
        let response = self.owner.update(cx, |owner, _| {
            owner
                .preferences_writer
                .submit(move || drafts::remove_exact(&draft))
        });
        let Ok(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ =
                this.update_in(cx, |this, _, cx| {
                    match result {
                        Ok(Ok(())) => {
                            this.saved.retain(|draft| draft.key() != key);
                            this.notice =
                                Some("The selected saved draft was discarded locally.".into());
                        }
                        _ => this.error = Some(
                            "Could not discard the selected draft; its stored text was preserved."
                                .into(),
                        ),
                    }
                    cx.notify();
                });
        })
        .detach();
    }
}
impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pending = self.pending;
        div().id("github-panel").max_h(px(620.)).overflow_y_scroll().flex().flex_col().gap_3()
            .child(label("github-worktree",format!("Local worktree: {}",self.repo.path().display())).text_color(rgb(p.muted)))
            .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending))).child(button("github-connect","Connect GitHub CLI account","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx)))))
            .child(div().flex().flex_wrap().gap_2()
                .child(button("github-sign-in-help","Copy sign-in command","",false).on_click(|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string("gh auth login --hostname github.com --web --skip-ssh-key".into()))))
                .child(button("github-refresh","Refresh PRs · network","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.refresh(1,window,cx))))
                .child(button("github-create","New pull request","plus",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.create(window,cx))))
                .child(button("github-disconnect","Disconnect GitTurtle","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.run(false,|_|SecureStore.remove(),|result,this,_,_|match result {Ok(())=>{this.account=None;this.notice=Some("GitTurtle authorization removed from Keychain. GitHub CLI and ordinary Git authentication keep their own settings.".into());},Err(error)=>this.error=Some(error.to_string())},window,cx)))))
            .when_some(self.account.as_ref(),|element,account|element.child(label("github-account",format!("Account: {account} · github.com"))))
            .when_some(self.notice.as_ref(),|element,notice|element.child(label("github-notice",notice.clone()).role(Role::Status).a11y_synthetic_children(native_accessibility::polite).text_color(rgb(p.muted))))
            .when_some(self.error.as_ref(),|element,error|element.child(label("github-error",error.clone()).role(Role::Alert).a11y_synthetic_children(native_accessibility::assertive).text_color(rgb(p.warning))))
            .when(pending,|element|element.child(div().flex().gap_2().child(label("github-busy",if self.writing {"Sending the reviewed GitHub action…"}else{"Loading the explicit request…"}).role(Role::Status).a11y_synthetic_children(|builder| {native_accessibility::polite(builder); builder.parent_node().set_busy();})).child(button("github-cancel","Cancel","",false).on_click(cx.listener(|this,_,_,cx|{if let Some(control)=&this.control{control.cancel();}
                    if let Some(control)=&this.local_control{control.cancel();}this.notice=Some("Cancellation requested. A submitted action may already have reached GitHub; it will not be replayed.".into());cx.notify();})))))
            .when(!self.create,|element|element.child(div().id("github-pr-list").max_h(px(175.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.rows.iter().map(|pull|{
                let pull=pull.clone();let selected=self.pull.as_ref().is_some_and(|p|p.number==pull.number);let text=format!("#{} · {} · {} · {} → {}",pull.number,if pull.draft {"Draft"}else{"Open"},pull.title,pull.head.branch,pull.base.branch);
                Button::new(("github-pr",pull.number)).ghost().w_full().label(text.clone()).accessibility_label(text).selected(selected).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.inspect(pull.clone(),window,cx)))
            })).when(self.rows.is_empty(),|element|element.child(label("github-list-empty","No pull requests loaded. Choose Refresh PRs to contact this repository.")))))
            .child(div().flex().gap_2().child(button("github-previous","Previous PR page","",false).disabled(pending||self.page<=1).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page-1,window,cx)))).child(label("github-page",format!("Page {} · up to {} PRs",self.page,github::PAGE_SIZE))).child(button("github-next","Next PR page","",false).disabled(pending||!self.has_next).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page+1,window,cx)))))
            .when_some(self.pull.as_ref(),|element,pull|element
                .child(label("github-captured-identities",format!("#{} · {}\nBefore {} ({})\nAfter {} ({})",pull.number,pull.title,pull.base.branch,pull.base.sha,pull.head.branch,pull.head.sha)))
                .child(button("github-open-changes","Open changes in native Compare","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.changes(window,cx)))))
            .when_some(self.description.as_ref(),|element,description|element.child(crate::editor_find::Editor::new(description).readonly(true).h(px(110.)).aria_label("Pull request description, exact Markdown source")))
            .when_some(self.status.as_ref(),|element,status|element.child(label("github-review-check-status",format!("Reviews: {}\nChecks: {}{}",if status.reviews.is_empty(){"None reported".into()}else{status.reviews.join(" · ")},if status.checks.is_empty(){"None reported".into()}else{status.checks.join(" · ")},if status.incomplete {"\nAdditional status entries are outside this 100-entry endpoint preview."}else{""}))))
            .when(self.create,|element|element
                .child(label("github-create-fields","Create a pull request · published head → destination base"))
                .child(Input::new(&self.title).aria_label("Pull request title").disabled(pending))
                .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.head).aria_label("Published head branch").disabled(pending))).child(div().flex_1().child(Input::new(&self.base).aria_label("Destination base branch").disabled(pending))))
                .child(div().flex().gap_2().children([(true,"Draft"),(false,"Ready")].map(|(draft,name)|button(name,name,"",self.draft==draft).toggled(self.draft==draft).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.draft=draft;this.confirm=None;this.save_draft(window,cx);}))))
                .child(button("github-load-templates","Load local base templates","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.load_templates(window,cx)))))
                .children(self.templates.iter().enumerate().map(|(index,template)|{let template=template.clone();button(("github-template",index),format!("Append {} · {}",template.path,&template.revision[..8]),"",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{let body=format!("{}{}{}",this.body.read(cx).value(),if this.body.read(cx).value().is_empty(){""}else{"\n\n"},template.body);this.body.update(cx,|input,cx|input.set_value(body,window,cx));this.save_draft(window,cx);} ))})))
            .when(self.create||self.pull.is_some(),|element|element
                .when(!self.create,|element|element.child(div().flex().flex_wrap().gap_2().child(button("github-discussion","Discussion comment","",self.discussion).toggled(self.discussion).disabled(pending).on_click(cx.listener(|this,_,_,cx|{this.discussion=true;this.confirm=None;cx.notify();}))).children([ReviewEvent::Comment,ReviewEvent::Approve,ReviewEvent::RequestChanges].map(|event|button(event.label(),format!("Review: {}",event.label()),"",!self.discussion&&self.event==event).toggled(!self.discussion&&self.event==event).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.discussion=false;this.event=event;this.confirm=None;this.save_draft(window,cx);}))))))
                .child(Textarea::new(&self.body).aria_label(if self.create {"Pull request description"}else{"Comment or review draft"}).disabled(pending))
                .child(label("github-draft-status",self.save_status.clone()).text_color(rgb(p.muted)))
                .child(button("github-review-submit","Review outbound action…","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.review_submission(window,cx)))))
            .when_some(self.confirm.as_ref(),|element,action|element.child(div().p_3().border_1().border_color(rgb(p.warning)).flex().flex_col().gap_2()
                .child(label("github-confirm-destination",format!("Send this action to {} · account {}\nThe captured text will be posted using the reviewed account. If an earlier attempt had an uncertain outcome, inspect GitHub before sending again.",action.action.destination(),action.account)))
                .child(button("github-confirm-send","Send to GitHub","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.submit(window,cx))))))
            .when(!self.attempts.is_empty(),|element|element.child(label("github-attempt-heading","Previous outbound attempts · refresh GitHub before repeating an interrupted action")).child(div().id("github-attempt-list").max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.attempts.iter().rev().take(10).enumerate().map(|(index,attempt)|label(("github-attempt",index),format!("{}: {}",attempt.destination,attempt.outcome))))))
            .when(!self.saved.is_empty(),|element|element.child(label("github-saved-heading","Recoverable local drafts · Copy retains older-head text without reattaching diff positions")).child(div().id("github-saved-drafts").max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.saved.iter().take(128).enumerate().map(|(index,draft)|{let draft=draft.clone(); let discard=draft.clone(); div().flex().gap_2().child(button(("github-copy-draft",index),format!("Copy {}",draft.key()),"",false).on_click(move|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(draft.text().to_owned())))).child(button(("github-discard-draft",index),"Discard saved draft","",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.discard_draft(discard.clone(),window,cx))))}))))
    }
}
