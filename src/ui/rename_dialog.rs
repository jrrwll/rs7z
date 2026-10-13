use super::Browser;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::{
    model::{
        archive::{ArchiveIndex, ArchiveOperation},
        file::{FileItem, FileKind},
    },
    service::{archive, operations},
};

pub(super) struct RenameDialog {
    item: FileItem,
    archive: Option<ArchiveIndex>,
    name: Entity<InputState>,
    error: Option<String>,
    browser: Entity<Browser>,
}

impl RenameDialog {
    pub(super) fn new(
        item: FileItem,
        archive: Option<ArchiveIndex>,
        browser: Entity<Browser>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cursor = if item.kind == FileKind::Directory {
            item.name.len()
        } else {
            item.name.rfind('.').unwrap_or(item.name.len())
        };
        let name = cx.new(|cx| {
            let mut input = InputState::new(window, cx).default_value(item.name.clone());
            input.set_selected_range(cursor..cursor, cx);
            input
        });
        Self {
            item,
            archive,
            name,
            error: None,
            browser,
        }
    }

    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().to_string();
        let result = if let Some(index) = &self.archive {
            archive::directory_name(&index.path, &self.item.path)
                .and_then(|entry| archive::rename_target(index, &entry, &name).map(|_| Some(entry)))
        } else {
            operations::rename_destination(&self.item.path, &name).map(|_| None)
        };
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return false;
            }
        };
        let source = self.item.path.clone();
        let archive_path = self.archive.as_ref().map(|index| index.path.clone());
        self.browser.update(cx, |browser, cx| {
            if let (Some(path), Some(entry)) = (archive_path, entry) {
                browser.run_archive_operation(
                    path,
                    ArchiveOperation::Rename { entry, name },
                    "Renaming archive entry",
                    window,
                    cx,
                );
            } else {
                browser.queue_progress(
                    "Renaming item",
                    false,
                    move |progress| operations::rename_with_progress(&source, &name, &progress),
                    window,
                    cx,
                );
            }
        });
        true
    }
}

impl Focusable for RenameDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

impl Render for RenameDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .min_w_0()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.item.name.clone()),
            )
            .child(div().text_sm().child("New name"))
            .child(Input::new(&self.name).id("rename-name").small())
            .when_some(self.error.clone(), |body, error| {
                body.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}
