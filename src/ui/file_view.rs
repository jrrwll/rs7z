use super::{Browser, Operation};
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::model::archive::ArchiveFormat;
use rs7z::model::file::{FileItem, FileKind};
use rs7z::service::{
    archive,
    file::{format_modified, format_size},
    logging,
};
use std::path::PathBuf;

#[derive(Clone, Copy)]
enum FileAction {
    Operation(Operation),
    Delete,
    Rename,
    Info,
    Refresh,
    Terminal,
}

// Headers and rows share column widths; long values stay inside their cells.
fn metadata_cell(width: Rems, label: impl Into<SharedString>) -> Div {
    div()
        .w(width)
        .min_w_0()
        .flex_shrink_0()
        .text_left()
        .truncate()
        .child(label.into())
}

fn menu_item(
    menu: PopupMenu,
    browser: Entity<Browser>,
    action: FileAction,
    label: &'static str,
    icon: IconName,
    enabled: bool,
) -> PopupMenu {
    let key_action: Option<Box<dyn Action>> = match action {
        FileAction::Operation(Operation::Add) => Some(Box::new(super::AddToArchive)),
        FileAction::Operation(Operation::Extract) => Some(Box::new(super::ExtractArchive)),
        FileAction::Operation(Operation::Test) => Some(Box::new(super::TestArchive)),
        FileAction::Operation(Operation::Copy) => Some(Box::new(super::CopyItems)),
        FileAction::Operation(Operation::Move) => Some(Box::new(super::MoveItems)),
        FileAction::Delete => Some(Box::new(super::DeleteItems)),
        FileAction::Rename => Some(Box::new(super::RenameItem)),
        FileAction::Info => Some(Box::new(super::ShowInfo)),
        FileAction::Refresh | FileAction::Terminal => None,
    };
    menu.item(
        PopupMenuItem::new(label)
            .icon(Icon::new(icon))
            .disabled(!enabled)
            .when_some(key_action, |item, action| item.action(action))
            .on_click(move |_, window, cx| {
                let browser = browser.clone();
                window.defer(cx, move |window, cx| {
                    browser.update(cx, |browser, cx| match action {
                        FileAction::Operation(operation) => {
                            browser.begin_operation(operation, window, cx)
                        }
                        FileAction::Delete => browser.delete_selected(window, cx),
                        FileAction::Rename => browser.begin_rename(window, cx),
                        FileAction::Info => browser.show_info(window, cx),
                        FileAction::Refresh => {
                            let path = browser.current_dir.clone();
                            browser.load_directory(&path, window, cx);
                        }
                        FileAction::Terminal => browser.open_in_terminal(window, cx),
                    });
                });
            }),
    )
}

fn file_context_menu(
    menu: PopupMenu,
    browser: Entity<Browser>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let state = browser.read(cx);
    let selected = state.selection.len();
    let can_operate = !state.busy;
    let has_item = can_operate && selected > 0;
    let can_add = state.can_add();
    let mutable = state.archive_mutable();
    let in_archive = state.archive.is_some();
    let has_single = state.selected_item().is_some();
    let can_extract_or_test = state.can_extract_or_test();
    let testable = state.is_testable();
    let can_open_terminal = state.can_open_terminal();
    let menu = menu.min_w(px(190.)).action_context(state.focus.clone());
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Operation(Operation::Add),
        "Add to archive…",
        IconName::Plus,
        can_add,
    );
    let menu = if in_archive {
        menu
    } else {
        menu_item(
            menu,
            browser.clone(),
            FileAction::Operation(Operation::Extract),
            "Extract archive…",
            IconName::ArchiveRestore,
            can_extract_or_test,
        )
    };
    let menu = if in_archive {
        menu
    } else {
        menu_item(
            menu,
            browser.clone(),
            FileAction::Operation(Operation::Test),
            "Test archive…",
            IconName::CheckCheck,
            testable,
        )
    };
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Operation(Operation::Copy),
        "Copy…",
        IconName::Copy,
        has_item,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Operation(Operation::Move),
        "Move…",
        IconName::MoveRight,
        has_item && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Delete,
        "Delete",
        IconName::Trash,
        has_item && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Rename,
        "Rename…",
        IconName::Pencil,
        can_operate && has_single && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Info,
        "Info…",
        IconName::Info,
        state.can_show_info(),
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Refresh,
        "Refresh",
        IconName::RefreshCw,
        can_operate,
    );
    if in_archive {
        return menu;
    }
    menu_item(
        menu.separator(),
        browser,
        FileAction::Terminal,
        "Open in terminal",
        IconName::Terminal,
        can_open_terminal,
    )
}

impl Browser {
    pub(super) fn sidebar_button(
        &self,
        name: String,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.current_dir == path;
        Button::new(format!("favorite-{}", path.display()))
            .small()
            .ghost()
            .w_full()
            .selected(selected)
            .accessibility_label(name.clone())
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .justify_start()
                    .gap_2()
                    .child(Icon::new(IconName::Folder).small())
                    .child(div().min_w_0().truncate().child(name)),
            )
            .tooltip(path.display().to_string())
            .on_click(cx.listener(move |this, _, window, cx| this.navigate(&path, window, cx)))
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = v_flex()
            .w(px(190.))
            .h_full()
            .flex_shrink_0()
            .p_3()
            .gap_1()
            .border_r(px(1.))
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .pb_2()
                    .child("FAVORITES"),
            )
            .child(self.sidebar_button("Working directory".into(), self.starting_dir.clone(), cx));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            panel = panel.child(self.sidebar_button("Home".into(), home.clone(), cx));
            for name in ["Desktop", "Documents", "Downloads"] {
                let path = home.join(name);
                if path.is_dir() {
                    panel = panel.child(self.sidebar_button(name.into(), path, cx));
                }
            }
        }
        if let Some(root) = self.current_dir.ancestors().last() {
            panel = panel
                .child(
                    div()
                        .pt_4()
                        .pb_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("LOCATIONS"),
                )
                .child(self.sidebar_button("Computer".into(), root.to_path_buf(), cx));
        }
        panel
    }

    pub(super) fn render_file_row(
        &self,
        item: &FileItem,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = item.path.clone();
        let selected = self.selection.contains(&path);
        let kind = match item.kind {
            FileKind::Directory => "Folder",
            FileKind::Archive => match archive::format(&item.path) {
                Some(ArchiveFormat::Zip) if archive::is_split(&item.path) => "zip split",
                Some(ArchiveFormat::Zip) => "zip",
                Some(ArchiveFormat::Rar) => "rar",
                Some(ArchiveFormat::SevenZip) if archive::is_split(&item.path) => "7z.001",
                Some(ArchiveFormat::SevenZip) => "7z",
                Some(ArchiveFormat::TarGzip) => "tgz",
                Some(ArchiveFormat::TarBzip2) => "tbz2",
                Some(ArchiveFormat::TarXz) => "txz",
                Some(ArchiveFormat::TarLzma) => "tlz",
                Some(ArchiveFormat::TarZstd) => "tzst",
                Some(ArchiveFormat::TarCompress) => "taz",
                Some(ArchiveFormat::Stream(format)) => format.extension(),
                _ => "Archive",
            },
            FileKind::Other => "File",
        };
        let size = if item.kind == FileKind::Directory {
            "--".to_owned()
        } else {
            format_size(item.size)
        };
        let modified = format_modified(item.modified);
        let icon = match item.kind {
            FileKind::Directory => IconName::Folder,
            FileKind::Archive => IconName::FileArchive,
            FileKind::Other => IconName::File,
        };
        h_flex()
            .id(format!("file-{}", path.display()))
            .test_support()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .h(px(32.))
            .px_3()
            .gap_2()
            .justify_start()
            .text_left()
            .when(selected, |row| {
                row.bg(cx.theme().primary)
                    .text_color(cx.theme().primary_foreground)
            })
            .when(!selected, |row| row.hover(|row| row.bg(cx.theme().muted)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let path = path.clone();
                    move |this, _, window, cx| {
                        if !this.selection.contains(&path) {
                            this.selection.context_click(path.clone());
                            logging::detail(format!(
                                "Selected for context menu: {}",
                                path.display()
                            ));
                            cx.notify();
                        }
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if !event.standard_click() {
                    return;
                }
                let modifiers = event.modifiers();
                this.select_path(path.clone(), modifiers, cx);
                window.focus(&this.focus, cx);
                if event.click_count() > 1 && !modifiers.modified() {
                    this.open_selected(window, cx);
                }
                cx.notify();
            }))
            .child(Icon::new(icon).small().flex_shrink_0())
            .child(div().flex_1().min_w_0().truncate().child(item.name.clone()))
            .child(metadata_cell(rems(6.), size).text_sm())
            .child(metadata_cell(rems(6.), kind).text_sm())
            .child(metadata_cell(rems(10.), modified).text_sm())
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered = self.filtered();
        let count = filtered.len();
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .h(px(32.))
                    .px_3()
                    .gap_2()
                    .justify_start()
                    .text_left()
                    .border_b(px(1.))
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w_4().flex_shrink_0())
                    .child(div().flex_1().min_w_0().truncate().child("Name"))
                    .child(metadata_cell(rems(6.), "Size"))
                    .child(metadata_cell(rems(6.), "Type"))
                    .child(metadata_cell(rems(10.), "Modified")),
            )
            .child(
                div()
                    .id("file-list-area")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .track_focus(&self.focus)
                    .key_context(super::FILE_LIST_CONTEXT)
                    .on_action(cx.listener(|this, _: &super::AddToArchive, window, cx| {
                        this.begin_operation(Operation::Add, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::ExtractArchive, window, cx| {
                        this.begin_operation(Operation::Extract, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::TestArchive, window, cx| {
                        this.begin_operation(Operation::Test, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::CopyItems, window, cx| {
                        this.begin_operation(Operation::Copy, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::MoveItems, window, cx| {
                        this.begin_operation(Operation::Move, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::DeleteItems, window, cx| {
                        this.delete_selected(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::RenameItem, window, cx| {
                        this.begin_rename(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::ShowInfo, window, cx| {
                        this.show_info(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &super::SelectAllItems, _, cx| {
                        if !this.busy {
                            let visible = this
                                .filtered()
                                .iter()
                                .map(|item| item.path.clone())
                                .collect::<Vec<_>>();
                            this.selection.select_all(&visible);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        if event.standard_click() {
                            this.selection.clear();
                            logging::detail("Selection cleared");
                            window.focus(&this.focus, cx);
                            cx.notify();
                        }
                    }))
                    .when(count > 0, |panel| {
                        panel
                            .child(
                                uniform_list(
                                    "file-list",
                                    count,
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        let filtered = this.filtered();
                                        range
                                            .filter_map(|ix| filtered.get(ix))
                                            .map(|item| {
                                                this.render_file_row(item, cx).into_any_element()
                                            })
                                            .collect::<Vec<_>>()
                                    }),
                                )
                                .size_full()
                                .track_scroll(&self.list_scroll),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .bottom_0()
                                    .child(Scrollbar::vertical(&self.list_scroll)),
                            )
                    })
                    .when(count == 0, |panel| {
                        panel.child(div().p_4().text_color(cx.theme().muted_foreground).child(
                            if self.search.is_empty() {
                                "No items"
                            } else {
                                "No results"
                            },
                        ))
                    })
                    .context_menu({
                        let browser = cx.entity();
                        move |menu, _, cx| file_context_menu(menu, browser.clone(), cx)
                    }),
            )
    }
}
