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
    select::{Select, SelectEvent, SelectState},
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::{self, Language, tr};
use rs7z::service::config::{self, Preferences};
use rs7z::theme::{BUILTIN_THEMES, ThemeFile};
use std::path::PathBuf;

struct SettingsDialog {
    browser: Entity<Browser>,
    tab: usize,
    source: usize,
    builtin: Entity<SelectState<SearchableVec<String>>>,
    external_file: Entity<InputState>,
    theme_source: usize,
    theme: Entity<SelectState<SearchableVec<String>>>,
    theme_file: Entity<InputState>,
    terminal_source: usize,
    terminal: Entity<SelectState<SearchableVec<String>>>,
    terminal_command: Entity<InputState>,
    picking_language: bool,
    picking_theme: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsDialog {
    fn new(browser: Entity<Browser>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let loaded = i18n::settings_path().and_then(|path| i18n::read_settings(&path));
        let (source, builtin, path, mut error) = match loaded {
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
        let preferences = match config::preferences_from_env() {
            Ok(preferences) => preferences,
            Err(config_error) => {
                error = Some(config_error);
                Preferences::default()
            }
        };
        let theme_index = if let Some(id) = preferences.theme().strip_prefix("builtin:") {
            BUILTIN_THEMES
                .iter()
                .position(|(key, _)| *key == id)
                .map(|index| index + 1)
        } else if preferences.theme().is_empty() {
            Some(0)
        } else {
            Some(BUILTIN_THEMES.len() + 1)
        }
        .unwrap_or(0);
        let theme_source = usize::from(theme_index == BUILTIN_THEMES.len() + 1);
        let theme = cx.new(|cx| {
            let mut labels = vec![tr("System default")];
            labels.extend(BUILTIN_THEMES.iter().map(|(_, name)| (*name).to_owned()));
            SelectState::new(
                SearchableVec::new(labels),
                Some(IndexPath::new(if theme_source == 0 {
                    theme_index
                } else {
                    0
                })),
                window,
                cx,
            )
        });
        let theme_file = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(if theme_index == BUILTIN_THEMES.len() + 1 {
                    preferences.theme()
                } else {
                    ""
                })
                .placeholder(tr("Choose a TOML theme file"))
        });
        let terminal_names = config::terminal_names();
        let terminal_index = if !preferences.terminal_command().is_empty() {
            terminal_names.len() + 1
        } else {
            terminal_names
                .iter()
                .position(|name| *name == preferences.terminal())
                .map(|index| index + 1)
                .unwrap_or(0)
        };
        let terminal_source = usize::from(terminal_index == terminal_names.len() + 1);
        let terminal = cx.new(|cx| {
            let mut labels = vec![tr("Disabled")];
            labels.extend(terminal_names.iter().map(|name| (*name).to_owned()));
            SelectState::new(
                SearchableVec::new(labels),
                Some(IndexPath::new(if terminal_source == 0 {
                    terminal_index
                } else {
                    0
                })),
                window,
                cx,
            )
        });
        let terminal_command = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(preferences.terminal_command())
                .placeholder(tr("Terminal command with $1 for the folder"))
        });
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&external_file, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.error = None;
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(&theme_file, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.error = None;
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(&terminal_command, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.error = None;
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(&theme, |this, _, event, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                this.error = None;
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(&terminal, |this, _, event, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                this.error = None;
                cx.notify();
            }
        }));
        Self {
            browser,
            tab: 0,
            source,
            builtin,
            external_file,
            theme_source,
            theme,
            theme_file,
            terminal_source,
            terminal,
            terminal_command,
            picking_language: false,
            picking_theme: false,
            error,
            _subscriptions: subscriptions,
        }
    }

    fn browse_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking_language {
            return;
        }
        self.picking_language = true;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title(tr("Choose a language file"))
                .add_filter("TOML", &["toml"])
                .pick_file()
                .await;
            view.update_in(cx, |this, window, cx| {
                this.picking_language = false;
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

    fn browse_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking_theme {
            return;
        }
        self.picking_theme = true;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title(tr("Choose a theme file"))
                .add_filter("TOML", &["toml"])
                .pick_file()
                .await;
            view.update_in(cx, |this, window, cx| {
                this.picking_theme = false;
                if let Some(path) = picked {
                    this.theme_file.update(cx, |input, cx| {
                        input.set_value(path.path().display().to_string(), window, cx)
                    });
                    this.theme_source = 1;
                    this.error = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picking_language || self.picking_theme {
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
            let config_path = config::config_path()?;
            let theme_index = if self.theme_source == 1 {
                BUILTIN_THEMES.len() + 1
            } else {
                self.theme
                    .read(cx)
                    .selected_index(cx)
                    .map(|index| index.row)
                    .unwrap_or(0)
            };
            let theme = match theme_index {
                0 => String::new(),
                index if index <= BUILTIN_THEMES.len() => {
                    let id = BUILTIN_THEMES[index - 1].0;
                    ThemeFile::builtin(id)?;
                    format!("builtin:{id}")
                }
                _ => {
                    let file = self.theme_file.read(cx).value().to_string();
                    if file.trim().is_empty() {
                        return Err(tr("Choose a theme file"));
                    }
                    let path = PathBuf::from(&file);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        config_path
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .join(path)
                    };
                    ThemeFile::load(&path)?;
                    file
                }
            };
            let terminal_index = if self.terminal_source == 1 {
                config::terminal_names().len() + 1
            } else {
                self.terminal
                    .read(cx)
                    .selected_index(cx)
                    .map(|index| index.row)
                    .unwrap_or(0)
            };
            let names = config::terminal_names();
            let (terminal, command) = match terminal_index {
                0 => (String::new(), String::new()),
                index if index <= names.len() => (names[index - 1].to_owned(), String::new()),
                _ => {
                    let command = self.terminal_command.read(cx).value().to_string();
                    if command.trim().is_empty() {
                        return Err(tr("Enter a terminal command containing $1"));
                    }
                    (String::new(), command)
                }
            };
            let preferences = Preferences::new(terminal, command, theme)?;
            let terminal = preferences.terminal_config()?;
            config::save_preferences(&config_path, &preferences)?;
            i18n::save_settings(&i18n::settings_path()?, &language)?;
            self.browser.update(cx, |browser, cx| {
                browser.terminal = terminal;
                cx.notify();
            });
            Ok(())
        });
        match result {
            Ok(()) => {
                window.push_notification(
                    tr("Settings saved. Restart rs7z to apply theme and language."),
                    cx,
                );
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
        let tab = self.tab;
        v_flex()
            .gap_4()
            .child(
                TabBar::new("settings-tabs")
                    .underline()
                    .w_full()
                    .selected_index(tab)
                    .child(Tab::new().label(tr("General")))
                    .child(Tab::new().label(tr("Advanced")))
                    .on_click(cx.listener(|this, index, _, cx| {
                        this.tab = *index;
                        this.error = None;
                        cx.notify();
                    })),
            )
            .when(tab == 0, |view| {
                view.child(
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
                                        .w(rems(11.))
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
                                                            .aria_label(tr(
                                                                "External language file",
                                                            ))
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
                                                        .disabled(self.picking_language)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.browse_language(window, cx)
                                                            },
                                                        )),
                                                ),
                                        ),
                                ),
                        ),
                )
            })
            .when(tab == 0, |view| {
                view.child(
                    GroupBox::new()
                        .id("settings-theme-section")
                        .title(tr("Theme"))
                        .outline()
                        .child(
                            h_flex()
                                .items_start()
                                .gap_4()
                                .child(
                                    RadioGroup::new("theme-source")
                                        .w(rems(11.))
                                        .flex_none()
                                        .child(
                                            Radio::new("theme-built-in")
                                                .small()
                                                .h_8()
                                                .items_center()
                                                .label(tr("Built-in theme")),
                                        )
                                        .child(
                                            Radio::new("theme-external")
                                                .small()
                                                .h_8()
                                                .items_center()
                                                .label(tr("External theme file")),
                                        )
                                        .selected_index(Some(self.theme_source))
                                        .on_change(cx.listener(|this, index, _, cx| {
                                            this.theme_source = *index;
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
                                            Select::new(&self.theme)
                                                .id("theme-choice")
                                                .accessibility_label(tr("Theme"))
                                                .small()
                                                .h_8()
                                                .w_full(),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    div().flex_1().min_w_0().child(
                                                        Input::new(&self.theme_file)
                                                            .id("external-theme-file")
                                                            .aria_label(tr("External theme file"))
                                                            .small()
                                                            .h_8(),
                                                    ),
                                                )
                                                .child(
                                                    Button::new("browse-theme")
                                                        .small()
                                                        .h_8()
                                                        .icon(IconName::FolderOpen)
                                                        .tooltip(tr("Browse…"))
                                                        .accessibility_label(tr("Browse…"))
                                                        .disabled(self.picking_theme)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.browse_theme(window, cx)
                                                            },
                                                        )),
                                                ),
                                        ),
                                ),
                        ),
                )
            })
            .when(tab == 1, |view| {
                view.child(
                    GroupBox::new()
                        .id("settings-terminal-section")
                        .title(tr("Terminal"))
                        .outline()
                        .child(
                            h_flex()
                                .items_start()
                                .gap_4()
                                .child(
                                    RadioGroup::new("terminal-source")
                                        .w(rems(11.))
                                        .flex_none()
                                        .child(
                                            Radio::new("terminal-built-in")
                                                .small()
                                                .h_8()
                                                .items_center()
                                                .label(tr("Built-in terminal")),
                                        )
                                        .child(
                                            Radio::new("terminal-custom")
                                                .small()
                                                .h_8()
                                                .items_center()
                                                .label(tr("Custom command")),
                                        )
                                        .selected_index(Some(self.terminal_source))
                                        .on_change(cx.listener(|this, index, _, cx| {
                                            this.terminal_source = *index;
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
                                            Select::new(&self.terminal)
                                                .id("terminal-choice")
                                                .accessibility_label(tr("Terminal"))
                                                .small()
                                                .h_8()
                                                .w_full(),
                                        )
                                        .child(
                                            Input::new(&self.terminal_command)
                                                .id("terminal-command")
                                                .aria_label(tr("Custom command"))
                                                .small()
                                                .h_8(),
                                        ),
                                ),
                        ),
                )
            })
            .when_some(self.error.clone(), |view, error| {
                view.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}

impl Browser {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let browser = cx.entity();
        let state = cx.new(|cx| SettingsDialog::new(browser, window, cx));
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
                            Button::new("apply-settings")
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
