use super::archive_dialog::ArchiveDialog;
use super::{
    Browser, Operation, operation_dialog,
    progress_dialog::{OperationProgressDialog, TestResult},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::{tr, trf};
use rs7z::model::archive::ArchiveOperation;
use rs7z::model::{file::FileKind, operations::OperationError};
use rs7z::service::{
    archive, config,
    file::{format_info_size, format_modified_seconds},
    hash::{self, HashAlgorithm, HashResult, HashTarget},
    logging,
    operations::{self, ProgressHandle},
};
use std::{path::PathBuf, time::Instant};

enum ConfirmTarget {
    Archive(Entity<ArchiveDialog>),
    Filesystem(Entity<operation_dialog::OperationDialog>),
}

struct ConfirmButton {
    target: ConfirmTarget,
    label: String,
    _subscription: Subscription,
}

impl ConfirmButton {
    fn archive(target: Entity<ArchiveDialog>, label: String, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&target, |_, _, cx| cx.notify());
        Self {
            target: ConfirmTarget::Archive(target),
            label,
            _subscription,
        }
    }

    fn filesystem(
        target: Entity<operation_dialog::OperationDialog>,
        label: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let _subscription = cx.observe(&target, |_, _, cx| cx.notify());
        Self {
            target: ConfirmTarget::Filesystem(target),
            label,
            _subscription,
        }
    }
}

impl Render for ConfirmButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = match &self.target {
            ConfirmTarget::Archive(state) => !state.read(cx).can_confirm(cx),
            ConfirmTarget::Filesystem(state) => !state.read(cx).can_confirm(cx),
        };
        let id = match self.target {
            ConfirmTarget::Archive(_) => "confirm-archive-operation",
            ConfirmTarget::Filesystem(_) => "confirm-operation",
        };
        Button::new(id)
            .label(self.label.clone())
            .primary()
            .disabled(disabled)
            .on_click(cx.listener(|this, _, window, cx| {
                let confirmed = match &this.target {
                    ConfirmTarget::Archive(state) => {
                        state.update(cx, |state, cx| state.confirm(window, cx))
                    }
                    ConfirmTarget::Filesystem(state) => {
                        state.update(cx, |state, cx| state.confirm(window, cx))
                    }
                };
                if confirmed {
                    window.close_dialog(cx);
                }
            }))
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn open_progress_dialog(
    title: &'static str,
    show_output: bool,
    dialog_state: Entity<OperationProgressDialog>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let finished = dialog_state.read(cx).is_finished();
        let has_test_result = dialog_state.read(cx).has_test_result();
        let cancel_state = dialog_state.clone();
        let escape_state = dialog_state.clone();
        dialog
            .title(if has_test_result {
                tr("Test results")
            } else {
                tr(title)
            })
            .width(window.rem_size() * if show_output { 38. } else { 34. })
            .child(dialog_state.clone())
            .close_button(has_test_result)
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
                        .label(tr(if finished { "Close" } else { "Cancel" }))
                        .on_click(move |_, window, cx| {
                            if cancel_state.read(cx).is_finished() {
                                window.close_dialog(cx);
                            } else {
                                cancel_state.update(cx, |state, cx| state.cancel(cx));
                            }
                        }),
                ),
            )
    });
}

fn hash_result_row(name: &'static str, value: String) -> impl IntoElement {
    let copy = value.clone();
    h_flex()
        .id(format!("checksum-row-{name}"))
        .test_support()
        .items_start()
        .w_full()
        .min_w_0()
        .gap_2()
        .text_left()
        .child(
            div()
                .id(format!("checksum-name-{name}"))
                .test_support()
                .w(rems(6.))
                .flex_shrink_0()
                .aria_label(name)
                .child(name),
        )
        .child(
            h_flex()
                .items_start()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    v_flex()
                        .id(format!("checksum-value-{name}"))
                        .test_support()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .aria_label(value.clone())
                        .children(
                            value.as_bytes().chunks(64).map(|chunk| {
                                div().child(String::from_utf8_lossy(chunk).into_owned())
                            }),
                        ),
                )
                .child(
                    Button::new(format!("copy-checksum-{name}"))
                        .small()
                        .ghost()
                        .icon(IconName::Copy)
                        .tooltip(trf("Copy %{name}", &[("name", name.to_string())]))
                        .accessibility_label(trf(
                            "Copy %{name} checksum",
                            &[("name", name.to_string())],
                        ))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                ),
        )
}

fn show_hash_result(result: HashResult, window: &mut Window, cx: &mut App) {
    let text = result.text();
    window.defer(cx, move |window, cx| {
        window.open_dialog(cx, move |dialog, window, _| {
            let copy = text.clone();
            dialog
                .title(tr("Checksums"))
                .width(window.rem_size() * 42.)
                .close_button(true)
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .id("checksum-target")
                                .test_support()
                                .w_full()
                                .min_w_0()
                                .text_sm()
                                .truncate()
                                .aria_label(result.label.clone())
                                .child(result.label.clone()),
                        )
                        .child(
                            v_flex()
                                .id("checksum-rows")
                                .test_support()
                                .w_full()
                                .min_w_0()
                                .max_h(rems(24.))
                                .gap_2()
                                .overflow_y_scrollbar()
                                .children(
                                    result
                                        .values
                                        .iter()
                                        .map(|(name, value)| hash_result_row(name, value.clone())),
                                ),
                        )
                        .child(
                            Button::new("copy-checksums")
                                .small()
                                .outline()
                                .icon(IconName::Copy)
                                .label(tr("Copy all"))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                                }),
                        ),
                )
                .footer(DialogFooter::new().child(
                    DialogClose::new().trigger(|button| button.label(tr("Close")).outline()),
                ))
        });
    });
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
                                    .aria_label(tr("Error details")),
                            )
                            .child(
                                Button::new("copy-error")
                                    .small()
                                    .outline()
                                    .icon(IconName::Copy)
                                    .label(tr("Copy all"))
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_message.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .ok_text(tr("OK"))
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
                        tr("Cannot read directory information"),
                        error.to_string(),
                        window,
                        cx,
                    );
                    return;
                }
            },
        };
        logging::info(format!("Showing info: {}", item.path.display()));
        let kind = tr(match item.kind {
            FileKind::Directory => "Folder",
            FileKind::Archive => rs7z::service::archive::format_name(&item.path),
            FileKind::Other => "File",
        });
        let size = if item.kind == FileKind::Directory {
            "--".to_owned()
        } else {
            format_info_size(item.size)
        };
        let modified = format_modified_seconds(item.modified);
        let plain = format!(
            "{}: {}\n{}: {kind}\n{}: {size}\n{}: {modified}\n{}: {}",
            tr("Name"),
            item.name,
            tr("Type"),
            tr("Size"),
            tr("Modified"),
            tr("Path"),
            item.path.display()
        );
        let html = format!(
            "<p><strong>{}:</strong> {}</p><p><strong>{}:</strong> {kind}</p><p><strong>{}:</strong> {size}</p><p><strong>{}:</strong> {modified}</p><p><strong>{}:</strong> {}</p>",
            tr("Name"),
            escape_html(&item.name),
            tr("Type"),
            tr("Size"),
            tr("Modified"),
            tr("Path"),
            escape_html(&item.path.display().to_string()),
        );
        window.open_dialog(cx, move |dialog, _, _| {
            let plain = plain.clone();
            dialog.title(tr("Properties")).child(
                v_flex()
                    .gap_3()
                    .child(TextView::html("file-properties", html.clone()).selectable(true))
                    .child(
                        Button::new("copy-properties")
                            .small()
                            .outline()
                            .icon(IconName::Copy)
                            .label(tr("Copy all"))
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
        if matches!(operation, Operation::Extract) && !self.can_extract_or_test() {
            return;
        }
        if matches!(operation, Operation::Test) && !self.is_testable() {
            return;
        }
        logging::info(format!("Preparing {}", operation.label()));
        if matches!(operation, Operation::Test) {
            let path = self
                .selected_item()
                .expect("testable archive is selected")
                .path
                .clone();
            self.run_operation(
                Operation::Test,
                vec![path],
                None,
                rs7z::model::archive::ArchiveOptions::default(),
                window,
                cx,
            );
            return;
        }
        if self.archive.is_some() {
            let browser = cx.entity();
            let state = cx.new(|cx| {
                super::archive_dialog::ArchiveDialog::new(operation, self, browser, window, cx)
            });
            let confirm_button =
                cx.new(|cx| ConfirmButton::archive(state.clone(), operation.confirm_label(), cx));
            let focus_state = state.clone();
            window.open_dialog(cx, move |dialog, window, _| {
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
                                    .trigger(|button| button.label(tr("Cancel")).outline()),
                            )
                            .child(confirm_button.clone()),
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
        let confirm_button =
            cx.new(|cx| ConfirmButton::filesystem(state.clone(), operation.confirm_label(), cx));
        let focus_state = state.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .title(operation.label())
                .width(if matches!(operation, Operation::Add) {
                    window.rem_size() * 46.
                } else {
                    px(540.)
                })
                .when(matches!(operation, Operation::Add), |dialog| {
                    dialog.h(window.rem_size() * 44.)
                })
                .child(state.clone())
                .on_ok(|_, _, _| false)
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(tr("Cancel")).outline()),
                        )
                        .child(confirm_button.clone()),
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
                .title(tr("Rename"))
                .width(window.rem_size() * 28.)
                .child(state.clone())
                .on_ok(move |_, window, cx| enter.update(cx, |state, cx| state.confirm(window, cx)))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(tr("Cancel")).outline()),
                        )
                        .child(
                            Button::new("confirm-rename")
                                .label(tr("Rename"))
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
        if matches!(operation, Operation::Test) && options.password.is_none() {
            let Some(path) = sources.first().cloned() else {
                self.start_operation(operation, sources, destination, options, window, cx);
                return;
            };
            self.busy = true;
            self.status = Some(tr("Checking archive…"));
            cx.notify();
            cx.spawn_in(window, async move |view, cx| {
                let encrypted = cx
                    .background_executor()
                    .spawn({
                        let path = path.clone();
                        async move { archive::read(&path).map(|index| index.encrypted) }
                    })
                    .await;
                view.update_in(cx, |browser, window, cx| {
                    browser.busy = false;
                    browser.status = None;
                    if encrypted == Ok(true) {
                        super::archive_password_dialog::ArchivePasswordDialog::open_for_test(
                            path,
                            cx.entity(),
                            window,
                            cx,
                        );
                    } else {
                        browser.start_operation(
                            operation,
                            sources,
                            destination,
                            options,
                            window,
                            cx,
                        );
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        self.start_operation(operation, sources, destination, options, window, cx);
    }

    fn start_operation(
        &mut self,
        operation: Operation,
        sources: Vec<PathBuf>,
        destination: Option<PathBuf>,
        options: rs7z::model::archive::ArchiveOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.queue_progress_with_test(
            title,
            show_output,
            if matches!(operation, Operation::Test) {
                sources
                    .first()
                    .cloned()
                    .map(|source| (source, options.clone()))
            } else {
                None
            },
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
                    self.show_error(tr("Cannot delete archive entries"), error, window, cx);
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
        self.queue_progress_with_test(title, show_output, None, work, window, cx);
    }

    fn queue_progress_with_test(
        &mut self,
        title: &'static str,
        show_output: bool,
        test_context: Option<(PathBuf, rs7z::model::archive::ArchiveOptions)>,
        work: impl FnOnce(ProgressHandle) -> Result<(), OperationError> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        logging::info(format!("{title} started"));
        self.busy = true;
        self.error = None;
        self.status = Some(format!("{}…", tr(title)));
        let browser = cx.entity();
        window.defer(cx, move |window, cx| {
            browser.update(cx, |browser, cx| {
                if test_context.is_some() {
                    browser.launch_progress_with_test(
                        title,
                        show_output,
                        test_context,
                        work,
                        window,
                        cx,
                    );
                } else {
                    browser.launch_progress(title, show_output, work, window, cx);
                }
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
        self.launch_progress_with_test(title, show_output, None, work, window, cx);
    }

    fn launch_progress_with_test(
        &mut self,
        title: &'static str,
        show_output: bool,
        test_context: Option<(PathBuf, rs7z::model::archive::ArchiveOptions)>,
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
        let started = Instant::now();
        let dialog_state = cx.new(|cx| {
            OperationProgressDialog::new(
                handle.clone(),
                (test_context.is_some()
                    || matches!(title, "Adding to archive" | "Extracting archive"))
                .then_some(started),
                window,
                cx,
            )
        });
        dialog_state.update(cx, |state, cx| state.start(window, cx));
        open_progress_dialog(title, show_output, dialog_state.clone(), window, cx);
        cx.spawn_in(window, async move |view, cx| {
            let result_handle = handle.clone();
            let (result, test_result) = cx
                .background_executor()
                .spawn(async move {
                    let result = work(handle);
                    let elapsed = started.elapsed();
                    let test_result = test_context.and_then(|(source, options)| {
                        if result == Err(OperationError::Cancelled) {
                            return None;
                        }
                        let index = archive::read_with_options(&source, &options).ok();
                        let error = result.as_ref().err().and_then(|error| match error {
                            OperationError::Failed(detail) => Some(detail.clone()),
                            OperationError::Cancelled => None,
                        });
                        Some(TestResult::new(&source, index.as_ref(), error, elapsed))
                    });
                    (result, test_result)
                })
                .await;
            view.update_in(cx, |this, window, cx| {
                let keep_dialog = show_output || test_result.is_some();
                dialog_state.update(cx, |state, cx| {
                    state.finish(&result, test_result, window, cx)
                });
                if !keep_dialog {
                    window.close_dialog(cx);
                }
                this.busy = false;
                if this.current_dir == started_dir {
                    this.load_directory(&started_dir, window, cx);
                }
                match result {
                    Ok(()) => {
                        logging::info(format!("{title} complete"));
                        this.status =
                            Some(trf("%{operation} complete", &[("operation", tr(title))]));
                    }
                    Err(OperationError::Cancelled) => {
                        logging::info(format!("{title} cancelled; partial changes remain"));
                        this.status = Some(trf(
                            "%{operation} cancelled; partial changes remain",
                            &[("operation", tr(title))],
                        ));
                        let item = result_handle.snapshot().item;
                        if item.starts_with("Partial output retained at:") {
                            this.show_error(tr("Extraction cancelled"), item, window, cx);
                        }
                    }
                    Err(OperationError::Failed(error)) => {
                        this.status = None;
                        if keep_dialog {
                            logging::error(&error);
                        } else {
                            this.show_error(
                                trf("%{operation} failed", &[("operation", tr(title))]),
                                error,
                                window,
                                cx,
                            );
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn start_hash(
        &mut self,
        algorithm: HashAlgorithm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let target = if let Some(index) = &self.archive {
            let entries = paths
                .iter()
                .map(|path| archive::directory_name(&index.path, path))
                .collect::<Result<Vec<_>, _>>();
            let entries = match entries {
                Ok(entries) => entries,
                Err(error) => {
                    self.show_error(tr("Cannot calculate checksums"), error, window, cx);
                    return;
                }
            };
            HashTarget::Archive {
                path: index.path.clone(),
                entries,
                password: self.archive_password.clone(),
            }
        } else {
            HashTarget::FileSystem(paths)
        };
        self.busy = true;
        self.error = None;
        self.status = Some(tr("Calculating checksums…"));
        let browser = cx.entity();
        window.defer(cx, move |window, cx| {
            browser.update(cx, |browser, cx| {
                browser.launch_hash(target, algorithm, window, cx)
            });
        });
        cx.notify();
    }

    fn launch_hash(
        &mut self,
        target: HashTarget,
        algorithm: HashAlgorithm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = ProgressHandle::default();
        let dialog_state =
            cx.new(|cx| OperationProgressDialog::new(handle.clone(), None, window, cx));
        dialog_state.update(cx, |state, cx| state.start(window, cx));
        open_progress_dialog(
            "Calculating checksums",
            false,
            dialog_state.clone(),
            window,
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { hash::calculate(target, algorithm, &handle) })
                .await;
            view.update_in(cx, |this, window, cx| {
                let completion = result.as_ref().map(|_| ()).map_err(|error| match error {
                    OperationError::Cancelled => OperationError::Cancelled,
                    OperationError::Failed(detail) => OperationError::Failed(detail.clone()),
                });
                dialog_state.update(cx, |state, cx| state.finish(&completion, None, window, cx));
                window.close_dialog(cx);
                this.busy = false;
                match result {
                    Ok(result) => {
                        this.status = Some(tr("Checksums calculated"));
                        show_hash_result(result, window, cx);
                    }
                    Err(OperationError::Cancelled) => {
                        this.status = Some(tr("Checksum calculation cancelled"));
                    }
                    Err(OperationError::Failed(error)) => {
                        this.status = None;
                        this.show_error(tr("Cannot calculate checksums"), error, window, cx);
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
            trf(
                "Delete \"%{name}\"?",
                &[(
                    "name",
                    paths[0]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                )],
            )
        } else {
            trf(
                "Delete %{count} selected items?",
                &[("count", count.to_string())],
            )
        };
        let description = if self.archive.is_some() {
            trf(
                "Permanently remove %{count} selected item(s) and their contents from this archive.",
                &[("count", count.to_string())],
            )
        } else if count == 1 {
            tr("This permanently removes the item from disk.")
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
                trf(" and %{count} more", &[("count", (count - 5).to_string())])
            } else {
                String::new()
            };
            trf(
                "Permanently remove %{names}%{more} from disk.",
                &[("names", names), ("more", more)],
            )
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title(title.clone())
                .description(description.clone())
                .confirm()
                .ok_text(tr("Delete"))
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
            self.show_error(tr("Cannot open terminal"), error, window, cx);
        } else {
            logging::info("Terminal launched");
        }
        cx.notify();
    }
}
