use super::{Browser, Operation};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::service::config;
use std::path::Path;

impl Browser {
    pub(super) fn toolbar_content(icon: IconName, label: &'static str) -> impl IntoElement {
        v_flex()
            .items_center()
            .gap_1()
            .child(Icon::new(icon).large())
            .child(div().text_xs().child(label))
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_item();
        let has_item = self.selection.len() > 0 && !self.busy;
        let has_single = selected.is_some() && !self.busy;
        let mutable = self.archive_mutable();
        let can_extract_or_test = self.can_extract_or_test();
        h_flex()
            .h(px(64.))
            .px_3()
            .gap_1()
            .border_b(px(1.))
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                Button::new("add")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Add to archive")
                    .disabled(!self.can_add())
                    .child(Self::toolbar_content(IconName::Plus, "Add…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.begin_operation(Operation::Add, window, cx)
                    })),
            )
            .when(self.archive.is_none(), |bar| bar.child(
                Button::new("extract")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Extract archive")
                    .disabled(!can_extract_or_test)
                    .child(Self::toolbar_content(IconName::ArchiveRestore, "Extract…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.begin_operation(Operation::Extract, window, cx)
                    })),
            ))
            .when(self.archive.is_none(), |bar| bar.child(
                Button::new("test")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Test archive")
                    .disabled(!self.is_testable())
                    .child(Self::toolbar_content(IconName::CheckCheck, "Test…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.begin_operation(Operation::Test, window, cx)
                    })),
            ))
            .child(
                Button::new("copy")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Copy item")
                    .disabled(!has_item)
                    .child(Self::toolbar_content(IconName::Copy, "Copy…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.begin_operation(Operation::Copy, window, cx)
                    })),
            )
            .child(
                Button::new("move")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Move item")
                    .disabled(!has_item || !mutable)
                    .child(Self::toolbar_content(IconName::MoveRight, "Move…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.begin_operation(Operation::Move, window, cx)
                    })),
            )
            .child(
                Button::new("delete")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Delete item")
                    .disabled(!has_item || !mutable)
                    .child(Self::toolbar_content(IconName::Trash, "Delete"))
                    .on_click(cx.listener(|this, _, window, cx| this.delete_selected(window, cx))),
            )
            .child(
                Button::new("rename")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Rename item")
                    .disabled(!has_single || !mutable)
                    .child(Self::toolbar_content(IconName::Pencil, "Rename…"))
                    .on_click(cx.listener(|this, _, window, cx| this.begin_rename(window, cx))),
            )
            .child(
                Button::new("info")
                    .small()
                    .ghost()
                    .h(px(54.))
                    .w(px(76.))
                    .accessibility_label("Show item information")
                    .disabled(!self.can_show_info())
                    .child(Self::toolbar_content(IconName::Info, "Info…"))
                    .on_click(cx.listener(|this, _, window, cx| this.show_info(window, cx))),
            )
            .child(div().flex_1())
            .when(self.archive.is_none(), |bar| bar.child(
                Button::new("terminal")
                    .small()
                    .ghost()
                    .icon(IconName::Terminal)
                    .tooltip(if config::is_available(&self.terminal) {
                        "Open folder in terminal"
                    } else {
                        "Configure custom_terminal or custom_terminal_cmd to open folders in a terminal"
                    })
                    .accessibility_label("Open in terminal")
                    .disabled(!self.can_open_terminal())
                    .on_click(cx.listener(|this, _, window, cx| this.open_in_terminal(window, cx))),
            ))
            .child(
                Button::new("refresh")
                    .small()
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .tooltip("Refresh")
                    .accessibility_label("Refresh")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let path = this.current_dir.clone();
                        this.load_directory(&path, window, cx);
                    })),
            )
    }

    pub(super) fn render_path_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(48.))
            .px_3()
            .gap_2()
            .border_b(px(1.))
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                Button::new("back")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip("Back")
                    .accessibility_label("Back")
                    .disabled(self.busy || self.history_index == 0)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.travel(this.history_index - 1, window, cx)
                    })),
            )
            .child(
                Button::new("forward")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronRight)
                    .tooltip("Forward")
                    .accessibility_label("Forward")
                    .disabled(self.busy || self.history_index + 1 >= self.history.len())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.travel(this.history_index + 1, window, cx)
                    })),
            )
            .child(
                Button::new("up")
                    .small()
                    .ghost()
                    .icon(IconName::ArrowUp)
                    .tooltip("Enclosing folder")
                    .accessibility_label("Enclosing folder")
                    .disabled(self.busy || self.current_dir.parent().is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(parent) = this.current_dir.parent().map(Path::to_path_buf) {
                            this.navigate(&parent, window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.))
                    .child(Input::new(&self.path_input).small()),
            )
            .child(
                Button::new("go")
                    .small()
                    .label("Go")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.go_to_path(window, cx))),
            )
            .child(
                div().w(px(180.)).child(
                    Input::new(&self.search_input)
                        .small()
                        .prefix(Icon::new(IconName::Search).small()),
                ),
            )
    }
}
