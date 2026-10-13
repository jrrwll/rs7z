use super::{Browser, Operation, operation_dialog, progress_dialog::OperationProgressDialog};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Sizable as _, WindowExt as _, v_flex};
use gpui_kit::*;
use rs7z::model::archive::ArchiveOperation;
use rs7z::model::{file::FileKind, operations::OperationError};
use rs7z::service::{
    archive, config,
    file::{format_info_size, format_modified_seconds},
    logging,
    operations::{self, ProgressHandle},
};
use std::path::PathBuf;

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

impl Browser {
    pub(super) fn show_error(
        &mut self,
        title: impl Into<String>,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        logging::error(&message);
        self.error = Some(message.clone());
        let title = title.into();
        let browser = cx.weak_entity();
        // Open after the current callback and any progress dialog dismissal finish.
        window.defer(cx, move |window, cx| {
            let details = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .default_value(message.clone())
                    .auto_grow(2, 12)
            });
            window.open_alert_dialog(cx, move |alert, window, _| {
                let browser = browser.clone();
                let copy_message = message.clone();
                alert
                    .title(title.clone())
                    .width(window.rem_size() * 34.)
                    .close_button(true)
                    .description(
                        v_flex()
                            .gap_3()
                            .child(
                                Textarea::new(&details)
                                    .readonly(true)
                                    .w_full()
                                    .accessibility_id("operation-error-message")
                                    .aria_label("Error details"),
                            )
                            .child(
                                Button::new("copy-error")
                                    .small()
                                    .outline()
                                    .icon(IconName::Copy)
                                    .label("Copy all")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_message.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .ok_text("OK")
                    .on_close(move |_, _, cx| {
                        let _ = browser.update(cx, |browser, cx| {
                            browser.error = None;
                            cx.notify();
                        });
                    })
            });
        });
        cx.notify();
    }

    pub(super) fn show_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_show_info() {
            return;
        }
        let item = match self.selected_item().cloned() {
            Some(item) => item,
            None => match rs7z::service::file::read_item(&self.current_dir) {
                Ok(item) => item,
                Err(error) => {
                    self.show_error(
                        "Cannot read directory information",
                        error.to_string(),
                        window,
                        cx,
                    );
                    return;
                }
            },
        };
        logging::info(format!("Showing info: {}", item.path.display()));
        let kind = match item.kind {
            FileKind::Directory => "Folder",
            FileKind::Archive => rs7z::service::archive::format_name(&item.path),
            FileKind::Other => "File",
        };
        let size = if item.kind == FileKind::Directory {
            "--".to_owned()
        } else {
            format_info_size(item.size)
        };
        let modified = format_modified_seconds(item.modified);
        let plain = format!(
            "Name: {}\nType: {kind}\nSize: {size}\nModified: {modified}\nPath: {}",
            item.name,
            item.path.display()
        );
        let html = format!(
            "<p><strong>Name:</strong> {}</p><p><strong>Type:</strong> {kind}</p><p><strong>Size:</strong> {size}</p><p><strong>Modified:</strong> {modified}</p><p><strong>Path:</strong> {}</p>",
            escape_html(&item.name),
            escape_html(&item.path.display().to_string()),
        );
        window.open_dialog(cx, move |dialog, _, _| {
            let plain = plain.clone();
            dialog.title("Properties").child(
                v_flex()
                    .gap_3()
                    .child(TextView::html("file-properties", html.clone()).selectable(true))
                    .child(
                        Button::new("copy-properties")
                            .small()
                            .outline()
                            .icon(IconName::Copy)
                            .label("Copy all")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(plain.clone()))
                            }),
                    ),
            )
        });
    }

    pub(super) fn begin_operation(
        &mut self,
        operation: Operation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if matches!(operation, Operation::Add) && !self.can_add() {
            return;
        }
        if matches!(operation, Operation::Move) && !self.archive_mutable() {
            return;
        }
        if matches!(operation, Operation::Copy | Operation::Move) && self.selection.len() == 0 {
            return;
        }
        if matches!(operation, Operation::Extract | Operation::Test) && !self.can_extract_or_test()
        {
            return;
        }
        if matches!(operation, Operation::Test) && !self.is_testable() {
            return;
        }
        logging::info(format!("Preparing {}", operation.label()));
        if self.archive.is_some() {
            let browser = cx.entity();
            let state = cx.new(|cx| {
                super::archive_dialog::ArchiveDialog::new(operation, self, browser, window, cx)
            });
            let focus_state = state.clone();
            window.open_dialog(cx, move |dialog, window, _| {
                let confirm = state.clone();
                let enter = state.clone();
                dialog
                    .title(operation.label())
                    .width(window.rem_size() * 34.)
                    .child(state.clone())
                    .on_ok(move |_, window, cx| {
                        enter.update(cx, |state, cx| state.confirm(window, cx))
                    })
                    .footer(
                        DialogFooter::new()
                            .child(
                                DialogClose::new()
                                    .trigger(|button| button.label("Cancel").outline()),
                            )
                            .child(
                                Button::new("confirm-archive-operation")
                                    .label(operation.confirm_label())
                                    .primary()
                                    .on_click(move |_, window, cx| {
                                        if confirm.update(cx, |state, cx| state.confirm(window, cx))
                                        {
                                            window.close_dialog(cx);
                                        }
                                    }),
                            ),
                    )
            });
            if matches!(operation, Operation::Copy | Operation::Move) {
                window.defer(cx, move |window, cx| {
                    focus_state.update(cx, |state, cx| state.focus_destination(window, cx));
                });
            }
            return;
        }
        let sources = self.selected_paths();
        self.open_filesystem_operation(operation, sources, window, cx);
    }

    pub(super) fn open_filesystem_operation(
        &mut self,
        operation: Operation,
        sources: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_dir = self.current_dir.clone();
        let browser = cx.entity();
        let state = cx.new(|cx| {
            operation_dialog::OperationDialog::new(
                operation,
                sources,
                current_dir,
                browser,
                window,
                cx,
            )
        });
        let focus_state = state.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let state_for_ok = state.clone();
            dialog
                .title(operation.label())
                .width(px(540.))
                .child(state.clone())
                .on_ok(|_, _, _| false)
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().trigger(|button| button.label("Cancel").outline()),
                        )
                        .child(
                            Button::new("confirm-operation")
                                .label(operation.confirm_label())
                                .primary()
                                .on_click(move |_, window, cx| {
                                    if state_for_ok
                                        .update(cx, |state, cx| state.confirm(window, cx))
                                    {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
        if matches!(operation, Operation::Extract) {
            // Focus after the dialog and its trigger have finished handling the click.
            window.defer(cx, move |window, cx| {
                focus_state.update(cx, |state, cx| state.focus_extract_destination(window, cx));
            });
        }
    }

    pub(super) fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.archive_mutable() {
            return;
        }
        let Some(item) = self.selected_item().cloned() else {
            return;
        };
        logging::info(format!("Preparing rename: {}", item.path.display()));
        let browser = cx.entity();
        let state = cx.new(|cx| {
            super::rename_dialog::RenameDialog::new(item, self.archive.clone(), browser, window, cx)
        });
        let focus = state.focus_handle(cx);
        window.open_dialog(cx, move |dialog, window, _| {
            let enter = state.clone();
            let confirm = state.clone();
            dialog
                .title("Rename")
                .width(window.rem_size() * 28.)
                .child(state.clone())
                .on_ok(move |_, window, cx| enter.update(cx, |state, cx| state.confirm(window, cx)))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().trigger(|button| button.label("Cancel").outline()),
                        )
                        .child(
                            Button::new("confirm-rename")
                                .label("Rename")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    if confirm.update(cx, |state, cx| state.confirm(window, cx)) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
        // Transfer focus after the trigger finishes handling its click.
        window.defer(cx, move |window, cx| focus.focus(window, cx));
    }

    pub(super) fn run_operation(
        &mut self,
        operation: Operation,
        sources: Vec<PathBuf>,
        destination: Option<PathBuf>,
        options: rs7z::model::archive::ArchiveOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let destination = destination.unwrap_or_default();
        logging::detail(format!(
            "{}: sources={sources:?}, destination={}",
            operation.label(),
            destination.display()
        ));
        let title = match operation {
            Operation::Add => "Adding to archive",
            Operation::Extract => "Extracting archive",
            Operation::Test => "Testing archive",
            Operation::Copy => "Copying items",
            Operation::Move => "Moving items",
        };
        let show_output = matches!(operation, Operation::Extract)
            && sources.first().is_some_and(|path| {
                archive::format(path) == Some(rs7z::model::archive::ArchiveFormat::Rar)
            });
        self.queue_progress(
            title,
            show_output,
            move |progress| match operation {
                Operation::Add => {
                    operations::add_many_with_options(&sources, &destination, &options, &progress)
                }
                Operation::Extract => sources
                    .first()
                    .ok_or_else(|| OperationError::Failed("Choose a ZIP or 7z archive.".into()))
                    .and_then(|source| {
                        operations::extract_with_options(source, &destination, &options, &progress)
                    }),
                Operation::Test => sources
                    .first()
                    .ok_or_else(|| OperationError::Failed("Choose a ZIP or 7z archive.".into()))
                    .and_then(|source| {
                        operations::test_archive_with_options(source, &options, &progress)
                    }),
                Operation::Copy | Operation::Move => operations::copy_or_move_with_progress(
                    &sources,
                    &destination,
                    matches!(operation, Operation::Move),
                    &progress,
                ),
            },
            window,
            cx,
        );
    }

    pub(super) fn start_delete(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if let Some(index) = &self.archive {
            let entries = paths
                .iter()
                .map(|path| archive::directory_name(&index.path, path))
                .collect::<Result<Vec<_>, _>>();
            match entries {
                Ok(entries) => self.run_archive_operation(
                    index.path.clone(),
                    ArchiveOperation::Delete { entries },
                    "Deleting archive entries",
                    window,
                    cx,
                ),
                Err(error) => {
                    self.show_error("Cannot delete archive entries", error, window, cx);
                }
            }
            return;
        }
        logging::detail(format!("Deleting paths: {paths:?}"));
        self.queue_progress(
            "Deleting items",
            false,
            move |progress| operations::delete_many_with_progress(&paths, &progress),
            window,
            cx,
        );
    }

    pub(super) fn run_archive_operation(
        &mut self,
        path: PathBuf,
        operation: ArchiveOperation,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_archive_operation_with_options(
            path,
            operation,
            Default::default(),
            title,
            window,
            cx,
        );
    }

    pub(super) fn run_archive_operation_with_options(
        &mut self,
        path: PathBuf,
        operation: ArchiveOperation,
        mut options: rs7z::model::archive::ArchiveOptions,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if options.password.is_none()
            && self
                .archive
                .as_ref()
                .is_some_and(|index| index.path == path)
        {
            options.password = self.archive_password.clone();
        }
        self.queue_progress(
            title,
            false,
            move |progress| archive::apply_with_options(&path, &operation, &options, &progress),
            window,
            cx,
        );
    }

    pub(super) fn queue_progress(
        &mut self,
        title: &'static str,
        show_output: bool,
        work: impl FnOnce(ProgressHandle) -> Result<(), OperationError> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        logging::info(format!("{title} started"));
        self.busy = true;
        self.error = None;
        self.status = Some(format!("{title}…"));
        let browser = cx.entity();
        window.defer(cx, move |window, cx| {
            browser.update(cx, |browser, cx| {
                browser.launch_progress(title, show_output, work, window, cx)
            });
        });
        cx.notify();
    }

    pub(super) fn launch_progress(
        &mut self,
        title: &'static str,
        show_output: bool,
        work: impl FnOnce(ProgressHandle) -> Result<(), OperationError> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started_dir = self.current_dir.clone();
        let handle = if show_output {
            ProgressHandle::with_output()
        } else {
            ProgressHandle::default()
        };
        let dialog_state = cx.new(|cx| OperationProgressDialog::new(handle.clone(), window, cx));
        dialog_state.update(cx, |state, cx| state.start(window, cx));
        window.open_dialog(cx, {
            let dialog_state = dialog_state.clone();
            move |dialog, window, cx| {
                let finished = dialog_state.read(cx).is_finished();
                let cancel_state = dialog_state.clone();
                let escape_state = dialog_state.clone();
                dialog
                    .title(title)
                    .width(window.rem_size() * if show_output { 38. } else { 29. })
                    .child(dialog_state.clone())
                    .close_button(false)
                    .overlay_closable(false)
                    .on_cancel(move |_, _, cx| {
                        if escape_state.read(cx).is_finished() {
                            return true;
                        }
                        escape_state.update(cx, |state, cx| state.cancel(cx));
                        false
                    })
                    .footer(
                        DialogFooter::new().child(
                            Button::new("cancel-progress")
                                .outline()
                                .label(if finished { "Close" } else { "Cancel" })
                                .on_click(move |_, window, cx| {
                                    if cancel_state.read(cx).is_finished() {
                                        window.close_dialog(cx);
                                    } else {
                                        cancel_state.update(cx, |state, cx| state.cancel(cx));
                                    }
                                }),
                        ),
                    )
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result_handle = handle.clone();
            let result = cx
                .background_executor()
                .spawn(async move { work(handle) })
                .await;
            view.update_in(cx, |this, window, cx| {
                dialog_state.update(cx, |state, cx| state.finish(&result, window, cx));
                if !show_output {
                    window.close_dialog(cx);
                }
                this.busy = false;
                if this.current_dir == started_dir {
                    this.load_directory(&started_dir, window, cx);
                }
                match result {
                    Ok(()) => {
                        logging::info(format!("{title} complete"));
                        this.status = Some(format!("{title} complete"));
                    }
                    Err(OperationError::Cancelled) => {
                        logging::info(format!("{title} cancelled; partial changes remain"));
                        this.status = Some(format!("{title} cancelled; partial changes remain"));
                        let item = result_handle.snapshot().item;
                        if item.starts_with("Partial output retained at:") {
                            this.show_error("Extraction cancelled", item, window, cx);
                        }
                    }
                    Err(OperationError::Failed(error)) => {
                        this.status = None;
                        if show_output {
                            logging::error(&error);
                        } else {
                            this.show_error(format!("{title} failed"), error, window, cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.archive_mutable() {
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        logging::info(format!("Confirming deletion of {count} item(s)"));
        let title = if count == 1 {
            format!(
                "Delete \"{}\"?",
                paths[0].file_name().unwrap_or_default().to_string_lossy()
            )
        } else {
            format!("Delete {count} selected items?")
        };
        let description = if self.archive.is_some() {
            format!(
                "Permanently remove {count} selected item(s) and their contents from this archive."
            )
        } else if count == 1 {
            "This permanently removes the item from disk.".to_owned()
        } else {
            let names = paths
                .iter()
                .take(5)
                .map(|path| {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>()
                .join(", ");
            let more = if count > 5 {
                format!(" and {} more", count - 5)
            } else {
                String::new()
            };
            format!("Permanently remove {names}{more} from disk.")
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title(title.clone())
                .description(description.clone())
                .confirm()
                .ok_text("Delete")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok({
                    let paths = paths.clone();
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.start_delete(paths.clone(), window, cx));
                        true
                    }
                })
        });
    }

    pub(super) fn open_in_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_open_terminal() {
            return;
        }
        let Some(path) = self.terminal_directory() else {
            return;
        };
        logging::info(format!("Opening terminal: {}", path.display()));
        if let Err(error) = config::open_directory(&self.terminal, path) {
            self.show_error("Cannot open terminal", error, window, cx);
        } else {
            logging::info("Terminal launched");
        }
        cx.notify();
    }
}
