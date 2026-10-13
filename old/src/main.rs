pub mod compress;
pub mod file;
pub mod util;

use gpui::*;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::table::{Column, Table, TableDelegate, TableEvent, TableState};
use gpui_component::{h_flex, v_flex, Root};
use std::path::{Path, PathBuf};

use compress::{list_archive_contents, list_archive_path_contents};
use file::{read_directory, FileItem};
use util::{format_size, format_timestamp};

fn is_archive_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        ext.as_str(),
        "zip" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "tbz2" | "txz" | "rar" | "zst"
    )
}

struct FileTableDelegate {
    files: Vec<FileItem>,
    columns: Vec<Column>,
    current_path: String,
    is_archive: bool,
    archive_path: String,
    archive_internal_path: String,
}

impl FileTableDelegate {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            columns: vec![
                Column::new("name", "Name").width(px(300.0)),
                Column::new("size", "Size").width(px(100.0)),
                Column::new("modified", "Modified").width(px(160.0)),
                Column::new("kind", "Kind").width(px(100.0)),
            ],
            current_path: String::new(),
            is_archive: false,
            archive_path: String::new(),
            archive_internal_path: String::new(),
        }
    }

    fn load_directory(&mut self, path: &str) -> bool {
        match read_directory(path.to_string()) {
            Ok(result) => {
                self.is_archive = result.is_archive;
                self.current_path = result.current_path.clone();
                self.files = result.files;
                self.archive_path = String::new();
                self.archive_internal_path = String::new();
                true
            }
            Err(_) => false,
        }
    }

    fn load_archive(&mut self, path: &str) -> bool {
        match list_archive_contents(path.to_string()) {
            Ok(result) => {
                self.is_archive = true;
                self.current_path = result.current_path.clone();
                self.files = result.files;
                self.archive_path = path.to_string();
                self.archive_internal_path = String::new();
                true
            }
            Err(_) => false,
        }
    }

    fn load_archive_path(&mut self, internal_path: &str) -> bool {
        match list_archive_path_contents(self.archive_path.clone(), internal_path.to_string()) {
            Ok(result) => {
                self.is_archive = true;
                self.current_path = result.current_path.clone();
                self.files = result.files;
                self.archive_internal_path = internal_path.to_string();
                true
            }
            Err(_) => false,
        }
    }
}

impl TableDelegate for FileTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.files.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        let file = &self.files[row_ix];

        let content = match col_ix {
            0 => {
                let icon = if file.kind == "folder" {
                    "📁 "
                } else if file.is_archive {
                    "📦 "
                } else {
                    "  "
                };
                format!("{}{}", icon, file.name)
            }
            1 => format_size(file.size),
            2 => format_timestamp(file.mtime),
            3 => file.kind.clone(),
            _ => String::new(),
        };

        h_flex().px_2().py_1().w_full().child(content)
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(gpui::rgb(0x888888))
            .child("No files found")
    }
}

struct MainWindow {
    table_state: Entity<TableState<FileTableDelegate>>,
    input_state: Entity<InputState>,
    current_path: String,
    is_archive: bool,
    file_count: usize,
    pending_browse_path: Option<String>,
}

impl MainWindow {
    fn new(
        initial_path: &str,
        is_archive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut delegate = FileTableDelegate::new();

        if is_archive {
            delegate.load_archive(initial_path);
        } else {
            delegate.load_directory(initial_path);
        }

        let current_path = delegate.current_path.clone();
        let file_count = delegate.files.len();

        let table_state = cx.new(|cx| TableState::new(delegate, window, cx));

        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(current_path.clone(), window, cx);
            state
        });

        cx.subscribe(&table_state, |this, _table_state, event, cx| {
            if let TableEvent::DoubleClickedRow(row_ix) = event {
                this.handle_double_click(*row_ix, cx);
            }
        })
        .detach();

        Self {
            table_state,
            input_state,
            current_path,
            is_archive,
            file_count,
            pending_browse_path: None,
        }
    }

    fn handle_double_click(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        let delegate = self.table_state.read(cx).delegate();

        if row_ix >= delegate.files.len() {
            return;
        }

        let file = &delegate.files[row_ix];

        if delegate.is_archive {
            if file.kind == "folder" {
                let path = file.path.clone();
                self.navigate_to_archive_path(&path, cx);
            }
        } else {
            if file.kind == "folder" {
                let path = file.path.clone();
                self.navigate_to_directory(&path, cx);
            } else if file.is_archive {
                let path = file.path.clone();
                self.navigate_to_archive(&path, cx);
            }
        }
    }

    fn navigate_to_path(&mut self, path: &str, cx: &mut Context<Self>) {
        let p = Path::new(path);

        if !p.exists() {
            self.navigate_to_parent(cx);
            return;
        }

        if p.is_dir() {
            if self.navigate_to_directory(path, cx) {
                return;
            }
        } else if p.is_file() && is_archive_extension(p) {
            if self.navigate_to_archive(path, cx) {
                return;
            }
        }

        self.navigate_to_parent(cx);
    }

    fn navigate_to_directory(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        self.table_state.update(cx, |state, cx| {
            if state.delegate_mut().load_directory(path) {
                self.current_path = state.delegate().current_path.clone();
                self.is_archive = state.delegate().is_archive;
                self.file_count = state.delegate().files.len();
                state.refresh(cx);
                cx.notify();
                true
            } else {
                false
            }
        })
    }

    fn navigate_to_archive(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        self.table_state.update(cx, |state, cx| {
            if state.delegate_mut().load_archive(path) {
                self.current_path = state.delegate().current_path.clone();
                self.is_archive = state.delegate().is_archive;
                self.file_count = state.delegate().files.len();
                state.refresh(cx);
                cx.notify();
                true
            } else {
                false
            }
        })
    }

    fn navigate_to_archive_path(&mut self, internal_path: &str, cx: &mut Context<Self>) -> bool {
        let archive_path = self.table_state.read(cx).delegate().archive_path.clone();

        self.table_state.update(cx, |state, cx| {
            state.delegate_mut().load_archive(&archive_path);
            if state.delegate_mut().load_archive_path(internal_path) {
                self.current_path = state.delegate().current_path.clone();
                self.is_archive = state.delegate().is_archive;
                self.file_count = state.delegate().files.len();
                state.refresh(cx);
                cx.notify();
                true
            } else {
                false
            }
        })
    }

    fn navigate_to_parent(&mut self, cx: &mut Context<Self>) {
        let current = if self.is_archive {
            self.table_state.read(cx).delegate().archive_path.clone()
        } else {
            self.current_path.clone()
        };

        let path = Path::new(&current);
        if let Some(parent) = path.parent() {
            let parent_path = parent.to_string_lossy().to_string();
            if !parent_path.is_empty() {
                self.navigate_to_directory(&parent_path, cx);
            }
        }
    }

    fn on_go_clicked(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let path = self.input_state.read(cx).text().to_string();
        self.navigate_to_path(&path, cx);
    }

    fn on_browse_clicked(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let current = if self.is_archive {
            self.table_state.read(cx).delegate().archive_path.clone()
        } else {
            self.current_path.clone()
        };

        let start_dir = if current.is_empty() || !Path::new(&current).exists() {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else {
            PathBuf::from(&current)
        };

        cx.spawn(async move |this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_directory(&start_dir)
                .pick_folder()
                .await;

            if let Some(folder) = result {
                let path = folder.path().to_string_lossy().to_string();
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.navigate_to_directory(&path, cx);
                    }).ok();
                }
            }
        })
        .detach();
    }

    fn on_up_clicked(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_to_parent(cx);
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(gpui::rgb(0xf5f5f5))
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .bg(gpui::rgb(0xe8e8e8))
                    .border_b_1()
                    .border_color(gpui::rgb(0xcccccc))
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .gap_2()
                            .child(Input::new(&self.input_state).h_6().flex_1())
                            .child(
                                Button::new("go")
                                    .child("Go")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_go_clicked(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("browse")
                                    .child("Browse")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_browse_clicked(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("up")
                                    .child("Up")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_up_clicked(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .px_4()
                    .py_1()
                    .bg(gpui::rgb(0xf0f0f0))
                    .border_b_1()
                    .border_color(gpui::rgb(0xdddddd))
                    .text_xs()
                    .text_color(gpui::rgb(0x666666))
                    .child(format!(
                        "{} items{}",
                        self.file_count,
                        if self.is_archive { " (Archive)" } else { "" }
                    )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(Table::new(&self.table_state)),
            )
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let initial_path = if args.len() > 1 {
        args[1].clone()
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    };

    let path = Path::new(&initial_path);
    let mut is_archive = false;

    if !path.exists() {
        eprintln!("Error: Path '{}' does not exist.", initial_path);
        std::process::exit(1);
    }

    if path.is_file() {
        if is_archive_extension(path) {
            is_archive = true;
        } else {
            eprintln!(
                "Error: '{}' is not a supported archive format.",
                initial_path
            );
            std::process::exit(1);
        }
    } else if !path.is_dir() {
        eprintln!(
            "Error: '{}' is not a valid file or directory.",
            initial_path
        );
        std::process::exit(1);
    }

    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        let path_for_window = initial_path.clone();
        let is_archive_for_window = is_archive;

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(900.0), px(600.0)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    MainWindow::new(&path_for_window, is_archive_for_window, window, cx)
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("Failed to open window");
    });
}
