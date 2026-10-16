use super::{Browser, Operation};
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, h_flex, v_flex,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    scroll::ScrollableElement as _,
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::{tr, trf};
use rs7z::{
    model::archive::{ArchiveDestination, ArchiveIndex, ArchiveOperation},
    model::file::FileKind,
    service::archive,
};
use std::path::PathBuf;

pub(super) struct ArchiveDialog {
    operation: Operation,
    archive: ArchiveIndex,
    entries: Vec<String>,
    sources: Vec<PathBuf>,
    directory: Entity<SelectState<SearchableVec<String>>>,
    directory_input: Entity<InputState>,
    archive_settings: super::archive_settings::ArchiveSettings,
    system_destination: bool,
    folder_input: Entity<InputState>,
    picking: bool,
    error: Option<String>,
    browser: Entity<Browser>,
    _subscriptions: Vec<Subscription>,
}

impl ArchiveDialog {
    pub(super) fn new(
        operation: Operation,
        state: &Browser,
        browser: Entity<Browser>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let index = state
            .archive
            .as_ref()
            .expect("archive dialog requires an archive");
        let archive = index.path.clone();
        let destination = if matches!(operation, Operation::Add) {
            state
                .selected_item()
                .filter(|item| item.kind == FileKind::Directory)
                .map(|item| &item.path)
                .unwrap_or(&state.current_dir)
        } else {
            &state.current_dir
        };
        let current = archive::directory_name(&archive, destination).unwrap_or_default();
        let entries = state
            .selected_paths()
            .iter()
            .filter_map(|path| archive::directory_name(&archive, path).ok())
            .collect();
        let directories: Vec<_> = archive::directories(index)
            .into_iter()
            .map(|name| format!("/{name}"))
            .collect();
        let selected = directories
            .iter()
            .position(|directory| directory == &format!("/{current}"))
            .unwrap_or(0);
        let directory = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(directories),
                Some(IndexPath::new(selected)),
                window,
                cx,
            )
            .searchable(true)
        });
        let directory_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("/{current}"))
                .placeholder(tr("/folder/new folder"))
        });
        let folder_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    archive
                        .parent()
                        .unwrap_or(std::path::Path::new("/"))
                        .display()
                        .to_string(),
                )
                .placeholder(tr("Destination folder path"))
        });
        let mut subscriptions = vec![
            cx.subscribe(&folder_input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&directory, window, |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.directory_input.update(cx, |input, cx| {
                        if input.value().as_str() != value {
                            input.set_value(value.clone(), window, cx);
                        }
                    });
                    this.error = None;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &directory_input,
                window,
                |this, input, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value();
                        let destination = archive::destination_directory(&this.archive, &value);
                        this.directory.update(cx, |select, cx| {
                            if let Ok(directory) = destination {
                                select.set_selected_value(&format!("/{directory}"), window, cx);
                            } else {
                                select.set_selected_index(None, window, cx);
                            }
                        });
                        this.error = None;
                        cx.notify();
                    }
                },
            ),
        ];
        let archive_settings = super::archive_settings::ArchiveSettings::new(window, cx);
        subscriptions.push(cx.subscribe_in(
            archive_settings.method_select(),
            window,
            |this, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    this.archive_settings.sync_method(window, cx);
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            archive_settings.compression_select(),
            |_, _, event, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    cx.notify();
                }
            },
        ));
        if let Some(password) = &state.archive_password {
            archive_settings.set_password(password, window, cx);
        }
        for input in archive_settings.password_inputs() {
            subscriptions.push(cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            }));
        }
        Self {
            operation,
            archive: index.clone(),
            entries,
            sources: Vec::new(),
            directory,
            directory_input,
            archive_settings,
            system_destination: matches!(operation, Operation::Copy | Operation::Move),
            folder_input,
            picking: false,
            error: None,
            browser,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn focus_destination(&self, window: &mut Window, cx: &mut Context<Self>) {
        let input = if self.system_destination {
            &self.folder_input
        } else {
            &self.directory_input
        };
        input.update(cx, |input, cx| {
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
            input.focus(window, cx);
        });
    }

    pub(super) fn can_confirm(&self, cx: &App) -> bool {
        !self.picking
            && self
                .archive_settings
                .password_error(
                    matches!(self.operation, Operation::Add),
                    self.archive.format,
                    cx,
                )
                .is_none()
    }

    fn system_folder(&self, cx: &App) -> PathBuf {
        let folder = PathBuf::from(self.folder_input.read(cx).value().to_string());
        if folder.is_absolute() {
            folder
        } else {
            self.archive
                .path
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .join(folder)
        }
    }

    fn pick(
        &mut self,
        folders: bool,
        destination: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picking {
            return;
        }
        self.picking = true;
        let directory = if destination {
            self.system_folder(cx)
        } else {
            self.archive
                .path
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .to_path_buf()
        };
        let directory = directory
            .ancestors()
            .find(|path| path.is_dir())
            .unwrap_or(std::path::Path::new("/"))
            .to_owned();
        cx.spawn_in(window, async move |view, cx| {
            let picker = rfd::AsyncFileDialog::new().set_directory(directory);
            let picked = if destination {
                picker
                    .set_title(tr("Choose destination folder"))
                    .pick_folder()
                    .await
                    .map(|path| vec![path])
            } else if folders {
                picker
                    .set_title(tr("Add folders to archive"))
                    .pick_folders()
                    .await
            } else {
                picker
                    .set_title(tr("Add files to archive"))
                    .pick_files()
                    .await
            };
            view.update_in(cx, |this, window, cx| {
                this.picking = false;
                if let Some(picked) = picked {
                    if destination {
                        if let Some(path) = picked.first() {
                            this.folder_input.update(cx, |input, cx| {
                                input.set_value(path.path().display().to_string(), window, cx)
                            });
                            this.focus_destination(window, cx);
                        }
                    } else {
                        for path in picked {
                            let path = path.path().to_path_buf();
                            if !this.sources.contains(&path) {
                                this.sources.push(path);
                            }
                        }
                    }
                    this.error = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picking {
            return false;
        }
        if self.archive.read_only
            && !(matches!(self.operation, Operation::Copy) && self.system_destination)
        {
            return self.invalid(
                &tr("This archive supports copying to a system folder only."),
                cx,
            );
        }
        let directory = self.directory_input.read(cx).value().to_string();
        if !self.system_destination
            && let Err(error) = archive::destination_directory(&self.archive, &directory)
        {
            return self.invalid(&error, cx);
        }
        let operation = if matches!(self.operation, Operation::Add) {
            if self.sources.is_empty() {
                return self.invalid(&tr("Choose files or folders to add."), cx);
            }
            ArchiveOperation::Add {
                sources: self.sources.clone(),
                directory,
            }
        } else {
            if self.entries.is_empty() {
                return self.invalid(&tr("Select archive entries first."), cx);
            }
            let destination = if self.system_destination {
                let folder = self.folder_input.read(cx).value().to_string();
                if folder.is_empty() {
                    return self.invalid(&tr("Enter or choose a destination folder."), cx);
                }
                let folder = self.system_folder(cx);
                if !folder.is_dir() {
                    return self.invalid(&tr("Choose an existing destination folder."), cx);
                }
                ArchiveDestination::FileSystem(folder)
            } else {
                ArchiveDestination::Directory(directory)
            };
            ArchiveOperation::Transfer {
                entries: self.entries.clone(),
                destination,
                move_entries: matches!(self.operation, Operation::Move),
            }
        };
        let archive = self.archive.path.clone();
        if let Err(error) = self.archive_settings.validate_password(
            matches!(self.operation, Operation::Add),
            self.archive.format,
            cx,
        ) {
            self.archive_settings.focus_confirmation(window, cx);
            return self.invalid(&error, cx);
        }
        let options = self
            .archive_settings
            .options_for_format(self.archive.format, cx);
        let title = match self.operation {
            Operation::Add => "Adding to archive",
            Operation::Move => "Moving archive entries",
            _ => "Copying archive entries",
        };
        self.browser.update(cx, |browser, cx| {
            browser
                .run_archive_operation_with_options(archive, operation, options, title, window, cx)
        });
        true
    }

    fn invalid(&mut self, message: &str, cx: &mut Context<Self>) -> bool {
        self.error = Some(message.to_owned());
        cx.notify();
        false
    }
}

impl Render for ArchiveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let adding = matches!(self.operation, Operation::Add);
        let items: Vec<_> = if adding {
            self.sources
                .iter()
                .map(|path| path.display().to_string())
                .collect()
        } else {
            self.entries.clone()
        };
        v_flex()
            .gap_4()
            .min_w_0()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.archive.path.display().to_string()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(trf("Sources (%{count})", &[("count", items.len().to_string())])),
                    )
                    .child(
                        v_flex()
                            .id("archive-sources")
                            .max_h(rems(8.))
                            .overflow_y_scrollbar()
                            .when(items.is_empty(), |list| list.child(tr("No sources selected")))
                            .children(items.into_iter().map(|name| div().text_sm().child(name))),
                    )
                    .when(adding, |section| {
                        section.child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("archive-add-files")
                                        .small()
                                        .outline()
                                        .label(tr("Add files…"))
                                        .disabled(self.picking)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.pick(false, false, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("archive-add-folders")
                                        .small()
                                        .outline()
                                        .label(tr("Add folders…"))
                                        .disabled(self.picking)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.pick(true, false, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("archive-clear-sources")
                                        .small()
                                        .ghost()
                                        .label(tr("Clear"))
                                        .disabled(self.picking)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.sources.clear();
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child(tr("Destination")))
                    .when(!adding, |section| {
                        section.child(
                            RadioGroup::new("archive-destination-kind")
                                .selected_index(Some(usize::from(self.system_destination)))
                                .disabled(self.archive.read_only)
                                .child(Radio::new("archive-target").label(tr("This archive")).disabled(self.archive.read_only))
                                .child(Radio::new("filesystem-target").label(tr("System folder")))
                                .on_change(cx.listener(|this, index, window, cx| {
                                    this.system_destination = *index == 1 || this.archive.read_only;
                                    this.error = None;
                                    cx.notify();
                                    let state = cx.entity();
                                    window.defer(cx, move |window, cx| {
                                        state.update(cx, |state, cx| state.focus_destination(window, cx));
                                    });
                                })),
                        )
                    })
                    .when(!self.system_destination, |section| {
                        section
                            .child(
                                Input::new(&self.directory_input)
                                    .id("archive-directory-path")
                                    .small(),
                            )
                            .child(div().text_xs().child(tr("Or choose an existing directory")))
                            .child(
                                Select::new(&self.directory)
                                    .id("archive-directory")
                                    .accessibility_label(tr("Directory inside archive"))
                                    .placeholder(tr("Choose an existing directory"))
                                    .small(),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("Paths start at / (archive root). Missing directories are created.")),
                            )
                    })
                    .when(self.system_destination, |section| {
                        section.child(h_flex().gap_2()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.folder_input).id("archive-system-path").small(),
                                ),
                            )
                            .child(
                                Button::new("archive-system-folder")
                                    .small()
                                    .outline()
                                    .flex_none()
                                    .label(tr("Choose folder…"))
                                    .disabled(self.picking)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick(true, true, window, cx)
                                    })),
                            ))
                    }),
            )
            .child(self.archive_settings.render(adding, self.archive.format, cx))
            .when(self.archive.read_only, |body| {
                body.child(div().text_xs().text_color(cx.theme().muted_foreground)
                    .child(tr("This archive supports browsing and copying to a system folder. Extract to edit its contents.")))
            })
            .when_some(self.error.clone(), |body, error| {
                body.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}
