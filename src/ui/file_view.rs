use super::{Browser, Operation};
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::tr;
use rs7z::model::archive::ArchiveFormat;
use rs7z::model::file::{FileItem, FileKind};
use rs7z::service::{
    archive,
    file::{format_modified, format_size},
    hash::HashAlgorithm,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum FileColumn {
    Name,
    Size,
    Type,
    CompressedSize,
    Modified,
    Created,
    Accessed,
    Encrypted,
    Comment,
    Crc,
}

pub(super) struct ColumnResize {
    column: FileColumn,
    start_x: Pixels,
    start_width: f32,
}

const FILE_COLUMNS: &[FileColumn] = &[
    FileColumn::Name,
    FileColumn::Size,
    FileColumn::Type,
    FileColumn::Modified,
    FileColumn::Created,
    FileColumn::Accessed,
];
const ARCHIVE_COLUMNS: &[FileColumn] = &[
    FileColumn::Name,
    FileColumn::Size,
    FileColumn::Type,
    FileColumn::CompressedSize,
    FileColumn::Modified,
    FileColumn::Created,
    FileColumn::Accessed,
    FileColumn::Encrypted,
    FileColumn::Comment,
    FileColumn::Crc,
];

impl FileColumn {
    fn label(self) -> String {
        tr(match self {
            Self::Name => "Name",
            Self::Size => "Size",
            Self::Type => "Type",
            Self::CompressedSize => "Packed size",
            Self::Modified => "Modified",
            Self::Created => "Created",
            Self::Accessed => "Accessed",
            Self::Encrypted => "Encrypted",
            Self::Comment => "Comment",
            Self::Crc => "CRC32",
        })
    }

    fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Type => "type",
            Self::CompressedSize => "compressed-size",
            Self::Modified => "modified",
            Self::Created => "created",
            Self::Accessed => "accessed",
            Self::Encrypted => "encrypted",
            Self::Comment => "comment",
            Self::Crc => "crc",
        }
    }

    fn default_width(self) -> f32 {
        match self {
            Self::Name => 300.,
            Self::Size | Self::CompressedSize => 110.,
            Self::Type | Self::Encrypted => 100.,
            Self::Modified | Self::Created | Self::Accessed => 144.,
            Self::Comment => 200.,
            Self::Crc => 110.,
        }
    }

    fn is_number(self) -> bool {
        matches!(self, Self::Size | Self::CompressedSize)
    }
}

fn metadata_cell(width: Pixels, label: impl Into<SharedString>, right_aligned: bool) -> Div {
    div()
        .w(width)
        .min_w_0()
        .flex_shrink_0()
        .when(right_aligned, |cell| cell.text_right())
        .truncate()
        .child(label.into())
}

fn menu_item(
    menu: PopupMenu,
    browser: Entity<Browser>,
    action: FileAction,
    label: String,
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
    window: &mut Window,
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
        tr("Add to archive…"),
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
            tr("Extract archive…"),
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
            tr("Test archive…"),
            IconName::CheckCheck,
            testable,
        )
    };
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Operation(Operation::Copy),
        tr("Copy…"),
        IconName::Copy,
        has_item,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Operation(Operation::Move),
        tr("Move…"),
        IconName::MoveRight,
        has_item && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Delete,
        tr("Delete"),
        IconName::Trash,
        has_item && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Rename,
        tr("Rename…"),
        IconName::Pencil,
        can_operate && has_single && mutable,
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Info,
        tr("Info…"),
        IconName::Info,
        state.can_show_info(),
    );
    let menu = menu_item(
        menu,
        browser.clone(),
        FileAction::Refresh,
        tr("Refresh"),
        IconName::RefreshCw,
        can_operate,
    );
    let can_hash = has_item
        && (in_archive
            || state.selected_paths().iter().all(|path| {
                state
                    .files
                    .iter()
                    .any(|item| &item.path == path && item.kind != FileKind::Directory)
            }));
    let hash_browser = browser.clone();
    let menu = if can_hash {
        menu.submenu("CRC", window, cx, move |submenu, _, _| {
            HashAlgorithm::MENU
                .iter()
                .fold(submenu, |submenu, &(algorithm, label)| {
                    let browser = hash_browser.clone();
                    submenu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let browser = browser.clone();
                        window.defer(cx, move |window, cx| {
                            browser.update(cx, |browser, cx| {
                                browser.start_hash(algorithm, window, cx)
                            });
                        });
                    }))
                })
        })
    } else if !has_item {
        menu.item(PopupMenuItem::new("CRC").disabled(true))
    } else {
        menu
    };
    if in_archive {
        return menu;
    }
    menu_item(
        menu.separator(),
        browser,
        FileAction::Terminal,
        tr("Open in terminal"),
        IconName::Terminal,
        can_open_terminal,
    )
}

impl Browser {
    fn visible_columns(&self) -> &'static [FileColumn] {
        if self.archive.is_some() {
            ARCHIVE_COLUMNS
        } else {
            FILE_COLUMNS
        }
    }

    fn column_width(&self, column: FileColumn) -> Pixels {
        px(*self
            .column_widths
            .get(&column)
            .unwrap_or(&column.default_width()))
    }

    fn file_table_width(&self) -> Pixels {
        self.visible_columns()
            .iter()
            .map(|column| self.column_width(*column))
            .sum::<Pixels>()
            + px(24.)
            + px(8.) * self.visible_columns().len().saturating_sub(1)
    }

    fn column_value(&self, item: &FileItem, column: FileColumn, kind: &str) -> String {
        let archive = item.archive.as_ref();
        match column {
            FileColumn::Name => item.name.clone(),
            FileColumn::Size => {
                if item.kind == FileKind::Directory {
                    "--".into()
                } else {
                    format_size(item.size)
                }
            }
            FileColumn::Type => kind.into(),
            FileColumn::CompressedSize => archive
                .and_then(|metadata| metadata.compressed_size)
                .map(format_size)
                .unwrap_or_else(|| "--".into()),
            FileColumn::Modified => format_modified(item.modified),
            FileColumn::Created => format_modified(item.created),
            FileColumn::Accessed => format_modified(item.accessed),
            FileColumn::Encrypted => archive
                .and_then(|metadata| metadata.encrypted)
                .map(|encrypted| tr(if encrypted { "Yes" } else { "No" }))
                .unwrap_or_else(|| "--".into()),
            FileColumn::Comment => archive
                .and_then(|metadata| metadata.comment.clone())
                .filter(|comment| !comment.is_empty())
                .unwrap_or_else(|| "--".into()),
            FileColumn::Crc => archive
                .and_then(|metadata| metadata.crc32)
                .map(|crc| format!("{crc:08X}"))
                .unwrap_or_else(|| "--".into()),
        }
    }

    fn render_file_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w(self.file_table_width())
            .h(px(32.))
            .px_3()
            .gap_2()
            .border_b(px(1.))
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .children(self.visible_columns().iter().map(|&column| {
                div()
                    .id(format!("column-{}", column.key()))
                    .test_support()
                    .relative()
                    .w(self.column_width(column))
                    .h_full()
                    .flex_shrink_0()
                    .child(
                        metadata_cell(
                            self.column_width(column),
                            column.label(),
                            column.is_number(),
                        )
                        .h_full()
                        .flex()
                        .items_center()
                        .when(column.is_number(), |cell| cell.justify_end()),
                    )
                    .child(
                        div()
                            .id(format!("resize-{}", column.key()))
                            .test_support()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(8.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.resizing_column = Some(ColumnResize {
                                        column,
                                        start_x: event.position.x,
                                        start_width: this.column_width(column).as_f32(),
                                    });
                                }),
                            ),
                    )
            }))
    }

    fn update_column_resize(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.resizing_column = None;
            return;
        }
        if let Some(resize) = &self.resizing_column {
            let width = (resize.start_width + (event.position.x - resize.start_x).as_f32())
                .clamp(60., 800.);
            self.column_widths.insert(resize.column, width);
            cx.notify();
        }
    }

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
                    .child(tr("FAVORITES")),
            );
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            panel = panel.child(self.sidebar_button(tr("Home"), home.clone(), cx));
            let movies = if cfg!(target_os = "macos") {
                "Movies"
            } else {
                "Videos"
            };
            for name in [
                "Desktop",
                "Documents",
                "Downloads",
                movies,
                "Music",
                "Pictures",
            ] {
                let path = home.join(name);
                if path.is_dir() {
                    panel = panel.child(self.sidebar_button(tr(name), path, cx));
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
                        .child(tr("LOCATIONS")),
                )
                .child(self.sidebar_button(tr("Computer"), root.to_path_buf(), cx));
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
        let kind = tr(kind);
        let icon = match item.kind {
            FileKind::Directory => IconName::Folder,
            FileKind::Archive => IconName::FileArchive,
            FileKind::Other => IconName::File,
        };
        h_flex()
            .id(format!("file-{}", path.display()))
            .test_support()
            .w(self.file_table_width())
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
            .children(self.visible_columns().iter().map(|&column| {
                let width = self.column_width(column);
                let value = self.column_value(item, column, &kind);
                if column == FileColumn::Name {
                    h_flex()
                        .w(width)
                        .min_w_0()
                        .flex_shrink_0()
                        .gap_2()
                        .child(Icon::new(icon).small().flex_shrink_0())
                        .child(div().min_w_0().truncate().child(value))
                        .into_any_element()
                } else {
                    metadata_cell(width, value.clone(), column.is_number())
                        .id(format!(
                            "file-cell-{}-{}",
                            item.path.display(),
                            column.key()
                        ))
                        .test_support()
                        .aria_label(value)
                        .text_sm()
                        .into_any_element()
                }
            }))
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered = self.filtered();
        let count = filtered.len();
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .bg(cx.theme().background)
            .on_mouse_move(cx.listener(|this, event, _, cx| {
                this.update_column_resize(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.resizing_column = None;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.resizing_column = None;
                }),
            )
            .child(
                div()
                    .id("file-horizontal-scroll")
                    .test_support()
                    .size_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&self.horizontal_scroll)
                    .child(
                        v_flex()
                            .w(self.file_table_width())
                            .h_full()
                            .child(self.render_file_header(cx))
                            .child(
                                div()
                                    .id("file-list-area")
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .relative()
                                    .track_focus(&self.focus)
                                    .key_context(super::FILE_LIST_CONTEXT)
                                    .on_action(cx.listener(
                                        |this, _: &super::AddToArchive, window, cx| {
                                            this.begin_operation(Operation::Add, window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::ExtractArchive, window, cx| {
                                            this.begin_operation(Operation::Extract, window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::TestArchive, window, cx| {
                                            this.begin_operation(Operation::Test, window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::CopyItems, window, cx| {
                                            this.begin_operation(Operation::Copy, window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::MoveItems, window, cx| {
                                            this.begin_operation(Operation::Move, window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::DeleteItems, window, cx| {
                                            this.delete_selected(window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::RenameItem, window, cx| {
                                            this.begin_rename(window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::ShowInfo, window, cx| {
                                            this.show_info(window, cx);
                                        },
                                    ))
                                    .on_action(cx.listener(
                                        |this, _: &super::SelectAllItems, _, cx| {
                                            if !this.busy {
                                                let visible = this
                                                    .filtered()
                                                    .iter()
                                                    .map(|item| item.path.clone())
                                                    .collect::<Vec<_>>();
                                                this.selection.select_all(&visible);
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .on_click(cx.listener(
                                        |this, event: &ClickEvent, window, cx| {
                                            if event.standard_click() {
                                                this.selection.clear();
                                                logging::detail("Selection cleared");
                                                window.focus(&this.focus, cx);
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .when(count > 0, |panel| {
                                        panel.child(
                                            uniform_list(
                                                "file-list",
                                                count,
                                                cx.processor(
                                                    |this, range: std::ops::Range<usize>, _, cx| {
                                                        let filtered = this.filtered();
                                                        range
                                                            .filter_map(|ix| filtered.get(ix))
                                                            .map(|item| {
                                                                this.render_file_row(item, cx)
                                                                    .into_any_element()
                                                            })
                                                            .collect::<Vec<_>>()
                                                    },
                                                ),
                                            )
                                            .size_full()
                                            .track_scroll(&self.list_scroll),
                                        )
                                    })
                                    .when(count == 0, |panel| {
                                        panel.child(
                                            div()
                                                .p_4()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if self.search.is_empty() {
                                                    tr("No items")
                                                } else {
                                                    tr("No results")
                                                }),
                                        )
                                    })
                                    .context_menu({
                                        let browser = cx.entity();
                                        move |menu, window, cx| {
                                            file_context_menu(menu, browser.clone(), window, cx)
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(32.))
                    .right_0()
                    .bottom_0()
                    .when(count > 0, |bar| {
                        bar.child(Scrollbar::vertical(&self.list_scroll))
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(Scrollbar::horizontal(&self.horizontal_scroll)),
            )
    }
}
