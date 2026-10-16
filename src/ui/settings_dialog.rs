use super::Browser;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    group_box::{GroupBox, GroupBoxVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::{self, Language, tr};
use std::path::PathBuf;

struct SettingsDialog {
    source: usize,
    builtin: Entity<SelectState<SearchableVec<String>>>,
    external_file: Entity<InputState>,
    picking: bool,
    error: Option<String>,
    _subscription: Subscription,
}

impl SettingsDialog {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let loaded = i18n::settings_path().and_then(|path| i18n::read_settings(&path));
        let (source, builtin, path, error) = match loaded {
            Ok(Language::English) => (0, 0, String::new(), None),
            Ok(Language::SimplifiedChinese) => (0, 1, String::new(), None),
            Ok(Language::External(path)) => {
                let error = i18n::validate_external(&path).err();
                (1, 0, path.display().to_string(), error)
            }
            Err(error) => (0, 0, String::new(), Some(error)),
        };
        let external_file = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(path)
                .placeholder(tr("Choose a TOML language file"))
        });
        let builtin = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![tr("English"), tr("Simplified Chinese")]),
                Some(IndexPath::new(builtin)),
                window,
                cx,
            )
        });
        let subscription = cx.subscribe(&external_file, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.error = None;
                cx.notify();
            }
        });
        Self {
            source,
            builtin,
            external_file,
            picking: false,
            error,
            _subscription: subscription,
        }
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title(tr("Choose a language file"))
                .add_filter("TOML", &["toml"])
                .pick_file()
                .await;
            view.update_in(cx, |this, window, cx| {
                this.picking = false;
                if let Some(path) = picked {
                    this.external_file.update(cx, |input, cx| {
                        input.set_value(path.path().display().to_string(), window, cx)
                    });
                    this.error = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picking {
            return false;
        }
        let language = if self.source == 0 {
            if self.builtin.read(cx).selected_index(cx) == Some(IndexPath::new(1)) {
                Ok(Language::SimplifiedChinese)
            } else {
                Ok(Language::English)
            }
        } else {
            let path = PathBuf::from(self.external_file.read(cx).value().to_string());
            i18n::validate_external(&path).map(Language::External)
        };
        let result = language.and_then(|language| {
            let path = i18n::settings_path()?;
            i18n::save_settings(&path, &language)
        });
        match result {
            Ok(()) => {
                window.push_notification(tr("Language saved. Restart rs7z to apply it."), cx);
                true
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                false
            }
        }
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                TabBar::new("settings-tabs")
                    .underline()
                    .w_full()
                    .selected_index(0)
                    .child(Tab::new().child(tr("General"))),
            )
            .child(
                GroupBox::new()
                    .id("settings-language-section")
                    .title(tr("Language"))
                    .outline()
                    .child(
                        h_flex()
                            .id("settings-language-choice")
                            .test_support()
                            .items_start()
                            .gap_4()
                            .child(
                                RadioGroup::new("language-source")
                                    .flex_none()
                                    .child(
                                        Radio::new("language-built-in")
                                            .small()
                                            .h_8()
                                            .items_center()
                                            .label(tr("Built-in language")),
                                    )
                                    .child(
                                        Radio::new("language-external")
                                            .small()
                                            .h_8()
                                            .items_center()
                                            .label(tr("External language file")),
                                    )
                                    .selected_index(Some(self.source))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        this.source = *index;
                                        this.error = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_3()
                                    .child(
                                        Select::new(&self.builtin)
                                            .id("built-in-language")
                                            .accessibility_label(tr("Built-in language"))
                                            .small()
                                            .h_8()
                                            .w_full(),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    Input::new(&self.external_file)
                                                        .id("external-language-file")
                                                        .aria_label(tr("External language file"))
                                                        .small()
                                                        .h_8(),
                                                ),
                                            )
                                            .child(
                                                Button::new("browse-language")
                                                    .small()
                                                    .h_8()
                                                    .icon(IconName::FolderOpen)
                                                    .tooltip(tr("Browse…"))
                                                    .accessibility_label(tr("Browse…"))
                                                    .disabled(self.picking)
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.browse(window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(self.error.clone(), |section, error| {
                        section.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    }),
            )
    }
}

impl Browser {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| SettingsDialog::new(window, cx));
        window.open_dialog(cx, move |dialog, window, _| {
            let confirm = state.clone();
            let enter = state.clone();
            dialog
                .title(tr("Settings"))
                .width(window.rem_size() * 32.)
                .child(state.clone())
                .on_ok(move |_, window, cx| enter.update(cx, |state, cx| state.apply(window, cx)))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(tr("Cancel")).outline()),
                        )
                        .child(
                            Button::new("apply-language")
                                .primary()
                                .label(tr("Apply"))
                                .on_click(move |_, window, cx| {
                                    if confirm.update(cx, |state, cx| state.apply(window, cx)) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }
}
