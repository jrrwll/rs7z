use super::Browser;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, v_flex,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    input::{Input, InputEvent, InputState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::model::{archive::ArchiveIndex, file::FileItem, operations::OperationError};
use rs7z::service::{archive, logging, operations::ProgressHandle};
use std::path::PathBuf;

pub(super) struct ArchiveNavigation {
    pub(super) path: PathBuf,
    pub(super) files: Vec<FileItem>,
    pub(super) index: ArchiveIndex,
    pub(super) record_history: bool,
    pub(super) history_index: Option<usize>,
}

pub(super) struct ArchivePasswordDialog {
    navigation: Option<ArchiveNavigation>,
    browser: Entity<Browser>,
    password: Entity<InputState>,
    progress: ProgressHandle,
    checking: bool,
    error: Option<String>,
    _subscription: Subscription,
}

impl ArchivePasswordDialog {
    pub(super) fn open(
        navigation: ArchiveNavigation,
        browser: Entity<Browser>,
        window: &mut Window,
        cx: &mut App,
    ) {
        logging::info(format!(
            "Password required to open archive: {}",
            navigation.index.path.display()
        ));
        let progress = ProgressHandle::default();
        let cancel = progress.clone();
        let restore = browser.clone();
        let state = cx.new(|cx| {
            let password = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("Archive password")
            });
            let subscription = cx.subscribe(&password, |this: &mut Self, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            });
            Self {
                navigation: Some(navigation),
                browser,
                password,
                progress,
                checking: false,
                error: None,
                _subscription: subscription,
            }
        });
        let focus = state.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let enter = state.clone();
            let confirm = state.clone();
            let cancel = cancel.clone();
            let restore = restore.clone();
            let checking = state.read(cx).checking;
            let empty = state.read(cx).password.read(cx).value().is_empty();
            dialog
                .title("Open encrypted archive")
                .width(window.rem_size() * 30.)
                .close_button(true)
                .child(state.clone())
                .on_ok(move |_, window, cx| {
                    enter.update(cx, |state, cx| state.confirm(window, cx));
                    false
                })
                .on_close(move |_, window, cx| {
                    cancel.cancel();
                    restore.update(cx, |browser, cx| {
                        browser.path_input.update(cx, |input, cx| {
                            input.set_value(browser.current_dir.display().to_string(), window, cx)
                        });
                    });
                })
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().trigger(|button| button.label("Cancel").outline()),
                        )
                        .child(
                            Button::new("unlock-archive")
                                .label("Open")
                                .primary()
                                .loading(checking)
                                .disabled(checking || empty)
                                .on_click(move |_, window, cx| {
                                    confirm.update(cx, |state, cx| state.confirm(window, cx))
                                }),
                        ),
                )
        });
        window.defer(cx, move |window, cx| {
            focus.update(cx, |state, cx| {
                state
                    .password
                    .update(cx, |input, cx| input.focus(window, cx))
            });
        });
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checking || self.progress.is_cancelled() {
            return;
        }
        let password = self.password.read(cx).value().to_string();
        if password.is_empty() {
            self.error = Some("Enter the archive password.".into());
            cx.notify();
            return;
        }
        let Some(navigation) = self.navigation.as_ref() else {
            return;
        };
        let path = navigation.index.path.clone();
        let destination = navigation.path.clone();
        let progress = self.progress.clone();
        self.checking = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    archive::verify_password(&path, &password, &progress)?;
                    let mut options = rs7z::model::archive::ArchiveOptions::default();
                    options.password = Some(password.clone());
                    let index = archive::read_with_options(&path, &options)?;
                    let directory = archive::directory_name(&path, &destination)?;
                    let files = archive::children(&index, &directory)?;
                    Ok::<_, OperationError>((password, index, files))
                })
                .await;
            let _ = view.update_in(cx, |this, window, cx| {
                if this.progress.is_cancelled() {
                    return;
                }
                this.checking = false;
                match result {
                    Ok((password, index, files)) => {
                        if let Some(mut navigation) = this.navigation.take() {
                            navigation.index = index;
                            navigation.files = files;
                            this.browser.update(cx, |browser, cx| {
                                browser.open_unlocked_archive(navigation, password, window, cx)
                            });
                            window.close_dialog(cx);
                        }
                    }
                    Err(OperationError::Failed(error)) => {
                        this.error = Some(error);
                        this.password.update(cx, |input, cx| {
                            input.set_selected_range(0..input.value().len(), cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    }
                    Err(OperationError::Cancelled) => {}
                }
            });
        })
        .detach();
    }
}

impl Render for ArchivePasswordDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .when_some(self.navigation.as_ref(), |body, navigation| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(navigation.index.path.display().to_string()),
                )
            })
            .child(div().text_sm().font_semibold().child("Password"))
            .child(
                Input::new(&self.password)
                    .id("archive-open-password")
                    .small()
                    .disabled(self.checking),
            )
            .when(self.checking, |body| {
                body.child(div().text_sm().child("Checking password…"))
            })
            .when_some(self.error.clone(), |body, error| {
                body.child(
                    div()
                        .id("archive-password-error")
                        .test_support()
                        .aria_label(error.clone())
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
    }
}
