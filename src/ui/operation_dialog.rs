use super::{Browser, Operation};
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, h_flex, v_flex,
};
use gpui_kit::component::{
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rfd::AsyncFileDialog;
use rs7z::service::operations::validate_item_name;
use rs7z::{
    model::archive::ArchiveFormat,
    service::{archive, zip::parse_volume_size},
};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy)]
enum PickTarget {
    SourceFile,
    SourceFolder,
    Destination,
}

const FORMATS: &[(&str, ArchiveFormat)] = &[
    ("ZIP", ArchiveFormat::Zip),
    ("7z", ArchiveFormat::SevenZip),
    ("tar.gz", ArchiveFormat::TarGzip),
    ("tar.bz2", ArchiveFormat::TarBzip2),
    ("tar.xz", ArchiveFormat::TarXz),
    ("tar.lzma", ArchiveFormat::TarLzma),
    ("tar.zst", ArchiveFormat::TarZstd),
];

pub(super) struct OperationDialog {
    operation: Operation,
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    destination_chosen: bool,
    destination_input: Entity<InputState>,
    name: Entity<InputState>,
    format: Entity<SelectState<SearchableVec<String>>>,
    volume_size: Entity<InputState>,
    archive_settings: super::archive_settings::ArchiveSettings,
    current_dir: PathBuf,
    browser: Entity<Browser>,
    picking: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl OperationDialog {
    pub(super) fn new(
        operation: Operation,
        sources: Vec<PathBuf>,
        current_dir: PathBuf,
        browser: Entity<Browser>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let destination = default_destination(operation, &sources, &current_dir);
        let default_name = if matches!(operation, Operation::Add) {
            destination
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive.zip".into())
        } else {
            sources
                .first()
                .map(|path| default_name(operation, path))
                .unwrap_or_default()
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(default_name));
        let format = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    FORMATS
                        .iter()
                        .map(|(label, _)| (*label).to_owned())
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let volume_size =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional, e.g. 100M or 1G"));
        let format_subscription =
            cx.subscribe_in(&format, window, |this: &mut Self, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    let Some((_, format)) = FORMATS.iter().find(|(label, _)| *label == value)
                    else {
                        return;
                    };
                    let current = this.name.read(cx).value().to_string();
                    let name = archive::with_format(Path::new(&current), *format);
                    this.name.update(cx, |input, cx| {
                        input.set_value(name.to_string_lossy().to_string(), window, cx)
                    });
                    this.error = None;
                    cx.notify();
                }
            });
        let destination_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(destination_text(operation, destination.as_deref()))
                .placeholder("Destination folder path")
        });
        let subscription = cx.subscribe(&destination_input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let displayed = destination_text(this.operation, this.destination.as_deref());
                if input.read(cx).value().as_str() != displayed {
                    this.destination_chosen = true;
                }
                this.error = None;
                cx.notify();
            }
        });
        Self {
            operation,
            sources,
            destination,
            destination_chosen: false,
            destination_input,
            name,
            format,
            volume_size,
            archive_settings: super::archive_settings::ArchiveSettings::new(window, cx),
            current_dir,
            browser,
            picking: false,
            error: None,
            _subscriptions: vec![subscription, format_subscription],
        }
    }

    pub(super) fn focus_extract_destination(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.destination_input.update(cx, |input, cx| {
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
            input.focus(window, cx);
        });
    }

    fn pick(&mut self, target: PickTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let typed_destination = self.typed_destination(cx);
        let directory = match target {
            PickTarget::Destination
                if matches!(self.operation, Operation::Add | Operation::Extract) =>
            {
                typed_destination.as_deref().unwrap_or(&self.current_dir)
            }
            PickTarget::Destination => self.destination.as_deref().unwrap_or(&self.current_dir),
            _ => self
                .sources
                .first()
                .map(PathBuf::as_path)
                .unwrap_or(&self.current_dir),
        };
        let directory = directory
            .ancestors()
            .find(|path| path.is_dir())
            .unwrap_or(&self.current_dir)
            .to_path_buf();
        let operation = self.operation;
        let suggested_name = if matches!(operation, Operation::Add) {
            Some(self.name.read(cx).value().to_string())
        } else if self.sources.len() == 1 {
            self.sources
                .first()
                .map(|source| default_name(operation, source))
        } else {
            None
        };
        cx.spawn_in(window, async move |view, cx| {
            let picker = AsyncFileDialog::new().set_directory(&directory);
            let picked = match target {
                PickTarget::SourceFile => {
                    let picker = if matches!(operation, Operation::Extract | Operation::Test) {
                        picker.add_filter("Archives", archive::OPEN_EXTENSIONS)
                    } else {
                        picker
                    };
                    if matches!(operation, Operation::Extract | Operation::Test) {
                        picker
                            .set_title("Choose archive")
                            .pick_file()
                            .await
                            .map(|file| vec![file])
                    } else {
                        picker.set_title("Choose source files").pick_files().await
                    }
                }
                PickTarget::SourceFolder => {
                    picker
                        .set_title("Choose source folders")
                        .pick_folders()
                        .await
                }
                PickTarget::Destination => match operation {
                    Operation::Add => picker
                        .set_title("Choose archive path")
                        .set_file_name(suggested_name.unwrap_or_else(|| "archive.zip".into()))
                        .add_filter("Archives", archive::SAVE_EXTENSIONS)
                        .save_file()
                        .await
                        .map(|file| vec![file]),
                    Operation::Extract | Operation::Copy | Operation::Move => picker
                        .set_title("Choose destination folder")
                        .pick_folder()
                        .await
                        .map(|folder| vec![folder]),
                    Operation::Test => None,
                },
            };
            view.update_in(cx, |this, window, cx| {
                this.picking = false;
                if let Some(handles) = picked {
                    let paths: Vec<_> = handles
                        .into_iter()
                        .map(|handle| handle.path().to_path_buf())
                        .collect();
                    match target {
                        PickTarget::SourceFile | PickTarget::SourceFolder => {
                            this.sources = paths;
                            if !this.destination_chosen && !matches!(this.operation, Operation::Add)
                            {
                                this.destination = default_destination(
                                    this.operation,
                                    &this.sources,
                                    &this.current_dir,
                                );
                                this.sync_destination_input(window, cx);
                            }
                            if !matches!(this.operation, Operation::Add) {
                                this.name.update(cx, |input, cx| {
                                    input.set_value(
                                        this.sources
                                            .first()
                                            .map(|path| default_name(this.operation, path))
                                            .unwrap_or_default(),
                                        window,
                                        cx,
                                    )
                                });
                            }
                        }
                        PickTarget::Destination => {
                            this.destination = paths.into_iter().next();
                            this.destination_chosen = true;
                            this.sync_destination_input(window, cx);
                            if matches!(this.operation, Operation::Add)
                                && let Some(name) =
                                    this.destination.as_ref().and_then(|path| path.file_name())
                            {
                                let name = name.to_string_lossy().into_owned();
                                this.name
                                    .update(cx, |input, cx| input.set_value(name, window, cx));
                                let selected = this
                                    .destination
                                    .as_ref()
                                    .and_then(|path| archive::format(path));
                                let value = FORMATS
                                    .iter()
                                    .find(|(_, format)| Some(*format) == selected)
                                    .map_or("ZIP", |(label, _)| *label);
                                this.format.update(cx, |select, cx| {
                                    select.set_selected_value(&value.to_owned(), window, cx)
                                });
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

    fn sync_destination_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = destination_text(self.operation, self.destination.as_deref());
        self.destination_input
            .update(cx, |input, cx| input.set_value(value, window, cx));
    }

    fn typed_destination(&self, cx: &App) -> Option<PathBuf> {
        let value = self.destination_input.read(cx).value();
        if value.trim().is_empty() {
            return None;
        }
        let path = PathBuf::from(value.as_str());
        Some(if path.is_absolute() {
            path
        } else {
            self.current_dir.join(path)
        })
    }

    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picking {
            return false;
        }
        if self.sources.is_empty() {
            self.error = Some("Choose a source file or folder.".into());
            cx.notify();
            return false;
        }
        if self.sources.iter().any(|source| !source.exists()) {
            self.error = Some("The selected source no longer exists.".into());
            cx.notify();
            return false;
        }
        if matches!(self.operation, Operation::Extract | Operation::Test)
            && (self.sources.len() != 1
                || !self.sources[0].is_file()
                || archive::format(&self.sources[0]).is_none())
        {
            self.error = Some(
                "Choose a supported archive (the first volume for .zip.001 or .7z.001 sets)."
                    .into(),
            );
            cx.notify();
            return false;
        }
        let destination = if matches!(self.operation, Operation::Test) {
            if archive::is_extract_only(&self.sources[0]) {
                self.error = Some(archive::extract_only_message(&self.sources[0]));
                cx.notify();
                return false;
            }
            None
        } else {
            let destination = if matches!(self.operation, Operation::Add | Operation::Extract) {
                self.typed_destination(cx)
            } else {
                self.destination.clone()
            };
            let Some(mut destination) = destination else {
                self.error = Some("Choose a destination.".into());
                cx.notify();
                return false;
            };
            if matches!(self.operation, Operation::Add) {
                if !destination.is_dir() {
                    self.error = Some("Enter or choose an existing folder in Save in.".into());
                    cx.notify();
                    return false;
                }
                let name = self.name.read(cx).value().to_string();
                if let Err(error) = validate_item_name(&name) {
                    self.error = Some(error);
                    cx.notify();
                    return false;
                }
                destination.push(name);
            }
            if matches!(self.operation, Operation::Copy | Operation::Move)
                && self.sources.len() == 1
            {
                let name = self.name.read(cx).value().to_string();
                if !matches!(
                    Path::new(&name).components().next(),
                    Some(Component::Normal(_))
                ) || Path::new(&name).components().count() != 1
                {
                    self.error = Some("Enter a single file or folder name.".into());
                    cx.notify();
                    return false;
                }
                destination.push(name);
            }
            if matches!(self.operation, Operation::Add)
                && (archive::format(&destination) != Some(self.archive_format(cx))
                    || archive::is_split(&destination))
            {
                self.error = Some(format!(
                    "Archive name must match the selected format (.{} or a supported alias).",
                    archive::extension(self.archive_format(cx))
                ));
                cx.notify();
                return false;
            }
            let valid_parent = destination.parent().is_some_and(Path::is_dir);
            if matches!(self.operation, Operation::Add) && !valid_parent {
                self.error = Some("Choose an archive path in an existing folder.".into());
                cx.notify();
                return false;
            }
            if matches!(self.operation, Operation::Extract)
                && !(destination.is_dir()
                    || (!destination.exists()
                        && destination
                            .ancestors()
                            .skip(1)
                            .find(|path| path.exists())
                            .is_some_and(Path::is_dir)))
            {
                self.error = Some("Choose a destination folder.".into());
                cx.notify();
                return false;
            }
            if matches!(self.operation, Operation::Extract) && archive::is_stream(&self.sources[0])
            {
                let result = archive::stream_output_name(&self.sources[0]).and_then(|name| {
                    let output = destination.join(name);
                    if output.symlink_metadata().is_ok() {
                        Err(format!(
                            "Output already exists: {}. Choose another destination folder.",
                            output.display()
                        ))
                    } else {
                        Ok(())
                    }
                });
                if let Err(error) = result {
                    self.error = Some(error);
                    cx.notify();
                    return false;
                }
            }
            if matches!(self.operation, Operation::Copy | Operation::Move) && self.sources.len() > 1
            {
                if !destination.is_dir() {
                    self.error = Some("Choose an existing destination folder.".into());
                    cx.notify();
                    return false;
                }
                if let Some(source) = self.sources.iter().find(|source| {
                    source
                        .file_name()
                        .is_some_and(|name| destination.join(name).exists())
                }) {
                    self.error = Some(format!(
                        "A destination already exists for {}.",
                        source.display()
                    ));
                    cx.notify();
                    return false;
                }
            }
            if matches!(self.operation, Operation::Copy | Operation::Move)
                && self.sources.len() == 1
                && (!valid_parent || destination.exists())
            {
                self.error = Some("Choose an unused name in an existing folder.".into());
                cx.notify();
                return false;
            }
            if self.sources.iter().any(|source| source == &destination) {
                self.error = Some("Source and destination must differ.".into());
                cx.notify();
                return false;
            }
            Some(destination)
        };
        let operation = self.operation;
        let sources = self.sources.clone();
        let mut options = self
            .archive_settings
            .options_for_format(self.archive_format(cx), cx);
        if matches!(self.operation, Operation::Add)
            && matches!(
                self.archive_format(cx),
                ArchiveFormat::Zip | ArchiveFormat::SevenZip
            )
        {
            match parse_volume_size(self.volume_size.read(cx).value().as_str()) {
                Ok(size) => options.volume_size = size,
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return false;
                }
            }
            if options.volume_size.is_some()
                && destination.as_ref().is_some_and(|path| path.exists())
            {
                self.error =
                    Some("Volumes can only be set when creating a new ZIP or 7z archive.".into());
                cx.notify();
                return false;
            }
        }
        self.browser.update(cx, |browser, cx| {
            browser.run_operation(operation, sources, destination, options, window, cx)
        });
        true
    }

    fn archive_format(&self, cx: &App) -> ArchiveFormat {
        if !matches!(self.operation, Operation::Add) {
            return self
                .sources
                .first()
                .and_then(|source| archive::format(source))
                .unwrap_or(ArchiveFormat::Zip);
        }
        self.format
            .read(cx)
            .selected_value()
            .and_then(|value| {
                FORMATS
                    .iter()
                    .find(|(label, _)| *label == value)
                    .map(|(_, format)| *format)
            })
            .unwrap_or(ArchiveFormat::Zip)
    }
}

impl Render for OperationDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sources = self
            .sources
            .iter()
            .map(|path| {
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(path.display().to_string())
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let destination = self
            .destination
            .as_ref()
            .map(|path| {
                if matches!(self.operation, Operation::Add) {
                    path.parent().unwrap_or(path).display().to_string()
                } else {
                    path.display().to_string()
                }
            })
            .unwrap_or_else(|| "No destination selected".into());
        let stream_name = matches!(self.operation, Operation::Extract)
            .then(|| {
                self.sources
                    .first()
                    .and_then(|path| archive::stream_output_name(path).ok())
            })
            .flatten();
        v_flex()
            .gap_4()
            .min_w_0()
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(if self.sources.is_empty() {
                                "Source".into()
                            } else {
                                format!("Sources ({})", self.sources.len())
                            }),
                    )
                    .child(
                        v_flex()
                            .max_h(px(140.))
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .when(self.sources.is_empty(), |list| {
                                list.child("No source selected")
                            })
                            .children(sources),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("choose-source-file")
                                    .small()
                                    .outline()
                                    .icon(IconName::File)
                                    .label(
                                        if matches!(
                                            self.operation,
                                            Operation::Extract | Operation::Test
                                        ) {
                                            "Choose archive…"
                                        } else {
                                            "Choose files…"
                                        },
                                    )
                                    .disabled(self.picking)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick(PickTarget::SourceFile, window, cx)
                                    })),
                            )
                            .when(
                                !matches!(self.operation, Operation::Extract | Operation::Test),
                                |row| {
                                    row.child(
                                        Button::new("choose-source-folder")
                                            .small()
                                            .outline()
                                            .icon(IconName::Folder)
                                            .label("Choose folders…")
                                            .disabled(self.picking)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.pick(PickTarget::SourceFolder, window, cx)
                                            })),
                                    )
                                },
                            ),
                    ),
            )
            .when(!matches!(self.operation, Operation::Test), |body| {
                body.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(
                            if matches!(self.operation, Operation::Add) {
                                "Save in"
                            } else {
                                "Destination folder"
                            },
                        ))
                        .when(matches!(self.operation, Operation::Add | Operation::Extract), |section| {
                            section.child(Input::new(&self.destination_input)
                                .id(if matches!(self.operation, Operation::Add) { "add-destination" } else { "extract-destination" }).small())
                                .child(div().text_xs().text_color(cx.theme().muted_foreground)
                                    .child("Enter a path or choose a folder. Relative paths start from the current folder."))
                        })
                        .when(!matches!(self.operation, Operation::Add | Operation::Extract), |section| section.child(
                            div()
                                .min_w_0()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(destination),
                        ))
                        .when_some(stream_name, |section, name| section.child(
                            div().min_w_0().text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Extracts one file: {name}. The source file is kept.")),
                        ))
                        .child(
                            Button::new("choose-destination")
                                .small()
                                .outline()
                                .icon(IconName::FolderOpen)
                                .label(if matches!(self.operation, Operation::Add) {
                                    "Choose archive…"
                                } else {
                                    "Choose folder…"
                                })
                                .disabled(self.picking)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick(PickTarget::Destination, window, cx)
                                })),
                        )
                        .when(
                            matches!(self.operation, Operation::Add)
                                || (matches!(self.operation, Operation::Copy | Operation::Move)
                                    && self.sources.len() == 1),
                            |section| {
                                section
                                    .child(div().text_sm().font_semibold().pt_2().child(
                                        if matches!(self.operation, Operation::Add) {
                                            "Archive name"
                                        } else {
                                            "Name at destination"
                                        },
                                    ))
                                    .child(Input::new(&self.name).id("operation-name").small())
                            },
                        ),
                )
            })
            .when(matches!(self.operation, Operation::Add), |body| {
                body.child(v_flex().gap_2()
                    .child(div().text_sm().font_semibold().child("Archive format"))
                    .child(Select::new(&self.format).id("archive-format").accessibility_label("Archive format").small())
                    .when(matches!(self.archive_format(cx), ArchiveFormat::Zip | ArchiveFormat::SevenZip), |section| {
                        section.child(div().text_sm().font_semibold().pt_2().child("Volume size (optional)"))
                            .child(Input::new(&self.volume_size).id("archive-volume-size").small())
                            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                                .child(if self.archive_format(cx) == ArchiveFormat::Zip {
                                    "B, K, M, G units; leave empty for one archive. Use a .zip name; open the resulting .zip.001 file."
                                } else {
                                    "B, K, M, G units; leave empty for one archive. Open the resulting .7z.001 file."
                                }))
                    }))
            })
            .when(
                matches!(
                    self.operation,
                    Operation::Add | Operation::Extract | Operation::Test
                ),
                |body| {
                    body.child(
                        self.archive_settings
                            .render(matches!(self.operation, Operation::Add), self.archive_format(cx), cx),
                    )
                },
            )
            .when_some(self.error.clone(), |body, error| {
                body.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}

fn destination_text(operation: Operation, destination: Option<&Path>) -> String {
    destination
        .map(|path| {
            if matches!(operation, Operation::Add) {
                path.parent().unwrap_or(path)
            } else {
                path
            }
            .display()
            .to_string()
        })
        .unwrap_or_default()
}

fn default_name(operation: Operation, source: &Path) -> String {
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    match operation {
        Operation::Add => format!(
            "{}.zip",
            source.file_stem().unwrap_or_default().to_string_lossy()
        ),
        Operation::Copy => format!("{name} copy"),
        Operation::Move => name.into_owned(),
        _ => String::new(),
    }
}

fn default_destination(
    operation: Operation,
    sources: &[PathBuf],
    current_dir: &Path,
) -> Option<PathBuf> {
    if matches!(operation, Operation::Add) && sources.is_empty() {
        return Some(current_dir.join("archive.zip"));
    }
    if sources.len() > 1 {
        return matches!(operation, Operation::Add).then(|| current_dir.join("archive.zip"));
    }
    let source = sources.first()?;
    match operation {
        Operation::Add => Some(source.with_file_name(default_name(operation, source))),
        Operation::Extract if archive::is_stream(source) => Some(
            source
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(current_dir)
                .to_path_buf(),
        ),
        Operation::Extract => Some(archive::extraction_directory(source)),
        Operation::Copy | Operation::Move => source.parent().map(Path::to_path_buf),
        Operation::Test => None,
    }
}
