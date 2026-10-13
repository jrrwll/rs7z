use super::{
    Browser,
    archive_password_dialog::{ArchiveNavigation, ArchivePasswordDialog},
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rs7z::model::archive::ArchiveIndex;
use rs7z::model::config::TerminalConfig;
use rs7z::model::file::{FileItem, FileKind};
use rs7z::service::{archive, config, file, logging};
use std::{
    fs,
    path::{Path, PathBuf},
};

impl Browser {
    pub(super) fn new_at(path: &Path, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut browser = Self::new(window, cx);
        browser.navigate(path, window, cx);
        browser
    }

    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let files = match file::read_directory(&current_dir) {
            Ok(files) => files,
            Err(error) => {
                logging::error(format!("Cannot read {}: {error}", current_dir.display()));
                Vec::new()
            }
        };
        let (terminal, config_error) = match config::from_env() {
            Ok(app) => (app, None),
            Err(error) => {
                logging::error(&error);
                (TerminalConfig::Disabled, Some(error))
            }
        };
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search files"));
        let path_input = cx
            .new(|cx| InputState::new(window, cx).default_value(current_dir.display().to_string()));
        let search_subscription =
            cx.subscribe_in(&search_input, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search = state.read(cx).value().to_string();
                    logging::detail(format!("Search changed: {:?}", this.search));
                    this.selection.clear();
                    cx.notify();
                }
            });
        let path_subscription =
            cx.subscribe_in(&path_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.go_to_path(window, cx);
                }
            });
        let mut browser = Self {
            current_dir: current_dir.clone(),
            starting_dir: current_dir.clone(),
            files,
            selection: Default::default(),
            history: vec![current_dir],
            history_index: 0,
            search: String::new(),
            search_input,
            path_input,
            busy: false,
            error: None,
            status: None,
            focus: cx.focus_handle().tab_stop(true),
            terminal,
            archive: None,
            archive_password: None,
            list_scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![search_subscription, path_subscription],
        };
        if let Some(error) = config_error {
            browser.show_error("Cannot load configuration", error, window, cx);
        }
        browser
    }

    pub(super) fn load_directory(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_location(path, false, None, window, cx);
    }

    fn load_location(
        &mut self,
        path: &Path,
        record_history: bool,
        history_index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if let Some(archive_path) = archive::archive_path(path) {
            if archive::is_extract_only(&archive_path) {
                if path == archive_path {
                    self.open_filesystem_operation(
                        super::Operation::Extract,
                        vec![archive_path],
                        window,
                        cx,
                    );
                } else {
                    self.show_error(
                        "Cannot browse this file",
                        archive::extract_only_message(&archive_path),
                        window,
                        cx,
                    );
                }
                return;
            }
            let path = path.to_owned();
            let mut options = rs7z::model::archive::ArchiveOptions::default();
            if self.archive.as_ref().is_some_and(|current| {
                fs::canonicalize(&archive_path).is_ok_and(|path| path == current.path)
            }) {
                options.password = self.archive_password.clone();
            }
            self.busy = true;
            if record_history || history_index.is_some() {
                self.status = Some("Reading archive…".into());
            }
            cx.notify();
            cx.spawn_in(window, async move |view, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let directory = archive::directory_name(&archive_path, &path)?;
                        let archive_path =
                            fs::canonicalize(archive::resolve_source(&archive_path)?)
                                .map_err(|e| e.to_string())?;
                        let index = archive::read_with_options(&archive_path, &options)?;
                        let files = if index.encrypted && options.password.is_none() {
                            // Encrypted 7z headers hide even the requested directory.
                            Vec::new()
                        } else {
                            archive::children(&index, &directory)?
                        };
                        let path = if directory.is_empty() {
                            archive_path
                        } else {
                            archive_path.join(directory)
                        };
                        Ok::<_, String>((path, files, index))
                    })
                    .await;
                view.update_in(cx, |this, window, cx| {
                    this.busy = false;
                    if record_history || history_index.is_some() {
                        this.status = None;
                    }
                    match result {
                        Ok((path, files, index)) => {
                            let unlocked = this
                                .archive
                                .as_ref()
                                .is_some_and(|current| current.path == index.path)
                                && this.archive_password.is_some();
                            if index.encrypted && !unlocked {
                                ArchivePasswordDialog::open(
                                    ArchiveNavigation {
                                        path,
                                        files,
                                        index,
                                        record_history,
                                        history_index,
                                    },
                                    cx.entity(),
                                    window,
                                    cx,
                                );
                            } else {
                                this.apply_location(
                                    path,
                                    files,
                                    Some(index),
                                    record_history,
                                    history_index,
                                    window,
                                    cx,
                                );
                            }
                        }
                        Err(error) => {
                            this.show_error("Cannot open archive", error, window, cx);
                        }
                    }
                })
                .ok();
            })
            .detach();
            return;
        }
        match fs::canonicalize(path)
            .and_then(|path| file::read_directory(&path).map(|files| (path, files)))
        {
            Ok((path, files)) => {
                self.apply_location(path, files, None, record_history, history_index, window, cx)
            }
            Err(error) => {
                self.show_error(
                    "Cannot open folder",
                    format!("Cannot open {}: {error}", path.display()),
                    window,
                    cx,
                );
            }
        }
    }

    pub(super) fn open_unlocked_archive(
        &mut self,
        navigation: ArchiveNavigation,
        password: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_location(
            navigation.path,
            navigation.files,
            Some(navigation.index),
            navigation.record_history,
            navigation.history_index,
            window,
            cx,
        );
        self.archive_password = Some(password);
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_location(
        &mut self,
        path: PathBuf,
        files: Vec<FileItem>,
        archive: Option<ArchiveIndex>,
        record_history: bool,
        history_index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        logging::info(format!("Opened directory: {}", path.display()));
        logging::detail(format!("Directory contains {} items", files.len()));
        if self.archive.as_ref().map(|index| &index.path)
            != archive.as_ref().map(|index| &index.path)
        {
            self.archive_password = None;
        }
        self.current_dir = path;
        self.files = files;
        self.archive = archive;
        self.selection.clear();
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        self.search.clear();
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.path_input.update(cx, |input, cx| {
            input.set_value(self.current_dir.display().to_string(), window, cx)
        });
        if record_history || history_index.is_some() {
            self.error = None;
        }
        if record_history && self.history[self.history_index] != self.current_dir {
            self.history.truncate(self.history_index + 1);
            self.history.push(self.current_dir.clone());
            self.history_index += 1;
        } else if let Some(index) = history_index {
            self.history_index = index;
        }
        cx.notify();
    }

    pub(super) fn navigate(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.load_location(path, true, None, window, cx);
    }

    pub(super) fn travel(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.history.get(index).cloned() {
            self.load_location(&path, false, Some(index), window, cx);
        }
    }

    pub(super) fn go_to_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(self.path_input.read(cx).value().to_string());
        let path = if path.is_absolute() {
            path
        } else {
            self.current_dir.join(path)
        };
        self.navigate(&path, window, cx);
    }

    pub(super) fn selected_item(&self) -> Option<&FileItem> {
        let path = self.selection.only()?;
        self.files.iter().find(|item| &item.path == path)
    }

    pub(super) fn can_extract_or_test(&self) -> bool {
        !self.busy
            && self.archive.is_none()
            && self
                .selected_item()
                .is_some_and(|item| item.kind == FileKind::Archive)
    }

    pub(super) fn is_testable(&self) -> bool {
        self.can_extract_or_test()
            && self
                .selected_item()
                .is_some_and(|item| !archive::is_extract_only(&item.path))
    }

    pub(super) fn can_add(&self) -> bool {
        !self.busy
            && self.archive_mutable()
            && (self.archive.is_none()
                || self.files.iter().all(|item| {
                    !self.selection.contains(&item.path) || item.kind == FileKind::Directory
                }))
    }

    pub(super) fn archive_mutable(&self) -> bool {
        self.archive.as_ref().is_none_or(|index| !index.read_only)
    }

    pub(super) fn can_show_info(&self) -> bool {
        !self.busy
            && (self.selected_item().is_some()
                || (self.archive.is_none() && self.selection.len() == 0))
    }

    pub(super) fn terminal_directory(&self) -> Option<&Path> {
        if self.busy || self.archive.is_some() {
            return None;
        }
        if self.selection.len() == 0 {
            return Some(&self.current_dir);
        }
        self.selected_item()
            .filter(|item| item.kind == FileKind::Directory)
            .map(|item| item.path.as_path())
    }

    pub(super) fn can_open_terminal(&self) -> bool {
        self.terminal_directory().is_some() && config::is_available(&self.terminal)
    }

    pub(super) fn selected_paths(&self) -> Vec<PathBuf> {
        let visible: Vec<_> = self
            .filtered()
            .iter()
            .map(|item| item.path.clone())
            .collect();
        self.selection.ordered(&visible)
    }

    pub(super) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };
        match item.kind {
            FileKind::Directory => self.navigate(&item.path, window, cx),
            FileKind::Archive => self.navigate(&item.path, window, cx),
            FileKind::Other => {}
        }
    }

    pub(super) fn filtered(&self) -> Vec<&FileItem> {
        let query = self.search.to_lowercase();
        self.files
            .iter()
            .filter(|item| item.name.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn step_selection(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let paths: Vec<_> = self
            .filtered()
            .into_iter()
            .map(|item| item.path.clone())
            .collect();
        self.selection.step(&paths, down, extend);
        if let Some(cursor) = self.selection.cursor()
            && let Some(ix) = paths.iter().position(|path| path == cursor)
        {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
        logging::detail(format!(
            "Selection changed: {} item(s)",
            self.selection.len()
        ));
        cx.notify();
    }

    pub(super) fn select_path(
        &mut self,
        path: PathBuf,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let visible: Vec<_> = self
            .filtered()
            .iter()
            .map(|item| item.path.clone())
            .collect();
        self.selection
            .click(&visible, path, modifiers.secondary(), modifiers.shift);
        logging::detail(format!(
            "Selection changed: {} item(s)",
            self.selection.len()
        ));
        cx.notify();
    }

    pub(super) fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.step_selection(true, event.keystroke.modifiers.shift, cx),
            "up" => self.step_selection(false, event.keystroke.modifiers.shift, cx),
            "enter" => self.open_selected(window, cx),
            "escape" => {
                self.selection.clear();
                cx.notify();
            }
            _ => {}
        }
    }
}
