mod actions;
mod archive_dialog;
mod archive_password_dialog;
mod archive_settings;
mod browser;
mod file_view;
mod operation_dialog;
mod progress_dialog;
mod rename_dialog;
mod selection;
mod shell;
mod toolbar;

#[cfg(test)]
#[path = "../../tests/support/file_view.rs"]
mod tests;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rs7z::model::archive::ArchiveIndex;
use rs7z::model::{config::TerminalConfig, file::FileItem};
use rs7z::service::{config, logging};
use std::path::PathBuf;

actions!(
    rs7z,
    [
        Quit,
        AddToArchive,
        ExtractArchive,
        TestArchive,
        CopyItems,
        MoveItems,
        DeleteItems,
        RenameItem,
        ShowInfo,
        SelectAllItems
    ]
);

const FILE_LIST_CONTEXT: &str = "Rs7zFileList";

fn init_app_actions(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| {
        logging::info("Application quit requested");
        cx.quit();
    });
    let shortcut = if cfg!(target_os = "macos") {
        "cmd-q"
    } else {
        "ctrl-q"
    };
    cx.bind_keys([KeyBinding::new(shortcut, Quit, None)]);
    // The key labelled Delete on a Mac keyboard produces Backspace.
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new(
        "backspace",
        DeleteItems,
        Some(FILE_LIST_CONTEXT),
    )]);
    cx.bind_keys([
        KeyBinding::new("alt-a", AddToArchive, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-e", ExtractArchive, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-t", TestArchive, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-c", CopyItems, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-m", MoveItems, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-r", RenameItem, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("alt-i", ShowInfo, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("delete", DeleteItems, Some(FILE_LIST_CONTEXT)),
        // Register last among DeleteItems bindings to show Alt+D in menus.
        KeyBinding::new("alt-d", DeleteItems, Some(FILE_LIST_CONTEXT)),
        KeyBinding::new("secondary-a", SelectAllItems, Some(FILE_LIST_CONTEXT)),
    ]);
    // Native menus capture shortcuts when installed, so bind the action first.
    cx.set_menus([Menu::new("rs7z").items([MenuItem::action("Quit rs7z", Quit)])]);
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Add,
    Extract,
    Test,
    Copy,
    Move,
}

impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Add => "Add to archive",
            Self::Extract => "Extract to",
            Self::Test => "Test archive",
            Self::Copy => "Copy to",
            Self::Move => "Move to",
        }
    }

    fn confirm_label(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Extract => "Extract",
            Self::Test => "Test",
            Self::Copy => "Copy",
            Self::Move => "Move",
        }
    }
}

struct Browser {
    current_dir: PathBuf,
    starting_dir: PathBuf,
    files: Vec<FileItem>,
    selection: selection::Selection,
    history: Vec<PathBuf>,
    history_index: usize,
    search: String,
    search_input: Entity<InputState>,
    path_input: Entity<InputState>,
    busy: bool,
    error: Option<String>,
    status: Option<String>,
    focus: FocusHandle,
    terminal: TerminalConfig,
    archive: Option<ArchiveIndex>,
    archive_password: Option<String>,
    list_scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

pub fn run(path: PathBuf) {
    match config::verbose_from_env() {
        Ok(verbose) => logging::set_verbose(verbose),
        Err(error) => logging::error(error),
    }
    logging::info("Application started");
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_kit::init(cx);
            init_app_actions(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1060.), px(660.)),
                    cx,
                ))),
                window_min_size: Some(size(px(760.), px(420.))),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Browser::new_at(&path, window, cx))
            })
            .expect("failed to open rs7z window");
        });
    logging::info("Application exited");
}
