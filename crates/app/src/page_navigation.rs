//! Page transitions share one retained repository. Inspections keep their
//! existing bounded context stacks; page visits never rebuild that workspace.
use crate::*;

pub(super) fn return_page(page: AppPage, origin: AppPage, repository: bool) -> AppPage {
    match page {
        AppPage::Settings if origin == AppPage::Projects => AppPage::Projects,
        _ if repository => AppPage::Repository,
        _ => AppPage::Projects,
    }
}

impl GitTurtle {
    pub(super) fn back_label(&self) -> &'static str {
        match self.page {
            AppPage::Settings => {
                match return_page(self.page, self.page_origin, self.repository.is_some()) {
                    AppPage::Projects => "Back to Projects",
                    _ => "Back to repository",
                }
            }
            AppPage::Projects => "Back to repository",
            AppPage::Repository if self.blame.is_visible() => "Back from Blame",
            AppPage::Repository if self.file_history.is_active() => "Back from File History",
            AppPage::Repository if self.revision_inspection.is_active() => "Back to inspection",
            AppPage::Repository if self.mode != WorkspaceMode::History => "Back to History",
            AppPage::Repository => "Back to Projects",
        }
    }

    pub(super) fn return_from_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = return_page(self.page, self.page_origin, self.repository.is_some());
        if self.page == AppPage::Repository {
            self.resume_file_history(window, cx);
            if self.mode != WorkspaceMode::History {
                self.ensure_editor(window, cx);
            }
            let focus = self.page_return_focus.take().unwrap_or_else(|| {
                if self.mode == WorkspaceMode::History {
                    self.focus.clone()
                } else {
                    self.file_focus.clone()
                }
            });
            let path = self.path.clone();
            let owner = cx.entity().downgrade();
            // Restore after the retained editor/list is attached to this frame.
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.page == AppPage::Repository && this.path == path {
                        focus.focus(window, cx);
                        window.refresh();
                    }
                });
            });
            self.try_automatic_refresh(window, cx);
        } else {
            self.app_focus.focus(window, cx);
        }
        self.repaint_page(window, cx);
    }

    pub(super) fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        if self.page != AppPage::Repository {
            self.return_from_page(window, cx);
        } else if self.mode == WorkspaceMode::History
            && !self.blame.is_visible()
            && !self.file_history.is_active()
            && !self.revision_inspection.is_active()
        {
            self.show_projects(window, cx);
        } else {
            self.back_to_history(window, cx);
            self.repaint_page(window, cx);
        }
    }

    pub(super) fn repaint_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A page replaces the entire element tree. Request a full native frame,
        // including after layout, so the old workspace cannot remain painted.
        cx.notify();
        window.refresh();
        window.on_next_frame(|window, _| window.refresh());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn settings_returns_to_the_page_it_was_opened_from() {
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Projects, true),
            AppPage::Projects
        );
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Repository, true),
            AppPage::Repository
        );
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Repository, false),
            AppPage::Projects
        );
    }

    #[test]
    fn project_hub_return_requires_a_resolved_repository() {
        assert_eq!(
            return_page(AppPage::Projects, AppPage::Settings, false),
            AppPage::Projects
        );
        assert_eq!(
            return_page(AppPage::Projects, AppPage::Settings, true),
            AppPage::Repository
        );
    }
}
