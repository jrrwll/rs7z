use super::{Browser, Operation};
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{
    Input, InputEvent, InputGroup, InputGroupAddon, InputGroupAddonAlignment, InputGroupButton,
    InputState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
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
use rs7z::i18n::{tr, trf};
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
    ("tar", ArchiveFormat::Tar),
    ("tar.gz", ArchiveFormat::TarGzip),
    ("tar.bz2", ArchiveFormat::TarBzip2),
    ("tar.xz", ArchiveFormat::TarXz),
    ("tar.lzma", ArchiveFormat::TarLzma),
    ("tar.zst", ArchiveFormat::TarZstd),
];

const VOLUME_SIZES: [(&str, &str); 9] = [
    ("10M", "10M"),
    ("100M", "100M"),
    ("1000M", "1000M"),
    ("650M - CD", "650M"),
    ("700M - CD", "700M"),
    ("4092M - FAT", "4092M"),
    ("4480M - DVD", "4480M"),
    ("8128M - DVD DL", "8128M"),
    ("23040M - BD", "23040M"),
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
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("Optional, e.g. 100M or 1G")));
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
                .placeholder(tr("Destination folder path"))
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
        let archive_settings = super::archive_settings::ArchiveSettings::new(window, cx);
        let method_subscription = cx.subscribe_in(
            archive_settings.method_select(),
            window,
            |this, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    this.archive_settings.sync_method(window, cx);
                    cx.notify();
                }
            },
        );
        let compression_subscription =
            cx.subscribe(archive_settings.compression_select(), |_, _, event, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    cx.notify();
                }
            });
        let password_subscriptions = archive_settings.password_inputs().map(|input| {
            cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            })
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
            archive_settings,
            current_dir,
            browser,
            picking: false,
            error: None,
            _subscriptions: vec![
                subscription,
                format_subscription,
                method_subscription,
                compression_subscription,
            ]
            .into_iter()
            .chain(password_subscriptions)
            .collect(),
        }
    }

    pub(super) fn can_confirm(&self, cx: &App) -> bool {
        !self.picking
            && self
                .archive_settings
                .password_error(
                    matches!(self.operation, Operation::Add),
                    self.archive_format(cx),
                    cx,
                )
                .is_none()
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
                        picker.add_filter(tr("Archives"), archive::OPEN_EXTENSIONS)
                    } else {
                        picker
                    };
                    if matches!(operation, Operation::Extract | Operation::Test) {
                        picker
                            .set_title(tr("Choose archive"))
                            .pick_file()
                            .await
                            .map(|file| vec![file])
                    } else {
                        picker
                            .set_title(tr("Choose source files"))
                            .pick_files()
                            .await
                    }
                }
                PickTarget::SourceFolder => {
                    picker
                        .set_title(tr("Choose source folders"))
                        .pick_folders()
                        .await
                }
                PickTarget::Destination => match operation {
                    Operation::Add => picker
                        .set_title(tr("Choose archive path"))
                        .set_file_name(suggested_name.unwrap_or_else(|| "archive.zip".into()))
                        .add_filter(tr("Archives"), archive::SAVE_EXTENSIONS)
                        .save_file()
                        .await
                        .map(|file| vec![file]),
                    Operation::Extract | Operation::Copy | Operation::Move => picker
                        .set_title(tr("Choose destination folder"))
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
            self.error = Some(tr("Choose a source file or folder."));
            cx.notify();
            return false;
        }
        if self.sources.iter().any(|source| !source.exists()) {
            self.error = Some(tr("The selected source no longer exists."));
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
                self.error = Some(tr("Choose a destination."));
                cx.notify();
                return false;
            };
            if matches!(self.operation, Operation::Add) {
                if !destination.is_dir() {
                    self.error = Some(tr("Enter or choose an existing folder in Save in."));
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
                    self.error = Some(tr("Enter a single file or folder name."));
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
                self.error = Some(tr("Choose an archive path in an existing folder."));
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
                self.error = Some(tr("Choose a destination folder."));
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
                    self.error = Some(tr("Choose an existing destination folder."));
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
                self.error = Some(tr("Choose an unused name in an existing folder."));
                cx.notify();
                return false;
            }
            if self.sources.iter().any(|source| source == &destination) {
                self.error = Some(tr("Source and destination must differ."));
                cx.notify();
                return false;
            }
            Some(destination)
        };
        let operation = self.operation;
        let sources = self.sources.clone();
        if let Err(error) = self.archive_settings.validate_password(
            matches!(operation, Operation::Add),
            self.archive_format(cx),
            cx,
        ) {
            self.error = Some(error);
            self.archive_settings.focus_confirmation(window, cx);
            cx.notify();
            return false;
        }
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

    fn render_creation(&self, cx: &Context<Self>) -> impl IntoElement {
        let format = self.archive_format(cx);
        let destination = v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child(tr("Save in")))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.destination_input)
                                .id("add-destination")
                                .aria_label(tr("Save in"))
                                .small(),
                        ),
                    )
                    .child(
                        Button::new("choose-destination")
                            .small()
                            .outline()
                            .flex_none()
                            .icon(IconName::FolderOpen)
                            .label(tr("Choose archive…"))
                            .disabled(self.picking)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pick(PickTarget::Destination, window, cx)
                            })),
                    ),
            )
            .child(
                div().text_xs().text_color(cx.theme().muted_foreground).child(tr(
                    "Enter a path or choose a folder. Relative paths start from the current folder.",
                )),
            );
        let identity = h_flex()
            .gap_4()
            .items_start()
            .child(
                v_flex()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().font_semibold().child(tr("Archive name")))
                    .child(
                        Input::new(&self.name)
                            .id("operation-name")
                            .aria_label(tr("Archive name"))
                            .small(),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .w_48()
                    .flex_none()
                    .child(div().text_sm().font_semibold().child(tr("Archive format")))
                    .child(
                        Select::new(&self.format)
                            .id("archive-format")
                            .accessibility_label(tr("Archive format"))
                            .small(),
                    ),
            );
        let compression = GroupBox::new()
            .id("archive-compression-settings")
            .title(tr("Compression"))
            .outline()
            .flex_1()
            .min_w_0()
            .child(
                self.archive_settings.render_compression(
                    format,
                    matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip)
                        .then(|| self.render_volume_size(format, cx).into_any_element()),
                    cx,
                ),
            );
        let encryption = GroupBox::new()
            .id("archive-encryption-settings")
            .title(tr("Encryption"))
            .outline()
            .flex_1()
            .min_w_0()
            .when(archive::supports_password(format), |section| {
                section.child(self.archive_settings.render_encryption(true, format, cx))
            })
            .when(!archive::supports_password(format), |section| {
                section.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("Encryption is not supported for this format.")),
                )
            });
        v_flex()
            .gap_4()
            .min_w_0()
            .child(destination)
            .child(identity)
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(compression)
                    .child(encryption),
            )
    }

    fn render_volume_size(&self, format: ArchiveFormat, cx: &App) -> impl IntoElement {
        let presets = InputGroupButton::new("archive-volume-size-presets")
            .icon(IconName::ChevronDown)
            .accessibility_label(tr("Choose volume size"))
            .tooltip(tr("Choose volume size"))
            .dropdown_menu({
                let input = self.volume_size.clone();
                move |menu, _, _| {
                    VOLUME_SIZES.into_iter().fold(menu, |menu, (label, size)| {
                        let input = input.clone();
                        menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            input.update(cx, |input, cx| input.set_value(size, window, cx));
                        }))
                    })
                }
            });
        v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child(tr("Volume size (optional)")))
            .child(
                InputGroup::new("archive-volume-size-group")
                    .small()
                    .input(Input::new(&self.volume_size).id("archive-volume-size"))
                    .addon(
                        InputGroupAddon::new("archive-volume-size-options")
                            .align(InputGroupAddonAlignment::InlineEnd)
                            .child(presets),
                    ),
            )
            .child(
                div().text_xs().text_color(cx.theme().muted_foreground)
                    .child(tr(if format == ArchiveFormat::Zip {
                        "B, K, M, G units; leave empty for one archive. Use a .zip name; open the resulting .zip.001 file."
                    } else {
                        "B, K, M, G units; leave empty for one archive. Open the resulting .7z.001 file."
                    })),
            )
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
            .unwrap_or_else(|| tr("No destination selected"));
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
                                tr("Source")
                            } else {
                                trf("Sources (%{count})", &[("count", self.sources.len().to_string())])
                            }),
                    )
                    .child(
                        v_flex()
                            .max_h(px(140.))
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .when(self.sources.is_empty(), |list| {
                                list.child(tr("No source selected"))
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
                                    .label(tr(
                                        if matches!(
                                            self.operation,
                                            Operation::Extract | Operation::Test
                                        ) {
                                            "Choose archive…"
                                        } else {
                                            "Choose files…"
                                        },
                                    ))
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
                                            .label(tr("Choose folders…"))
                                            .disabled(self.picking)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.pick(PickTarget::SourceFolder, window, cx)
                                            })),
                                    )
                                },
                            ),
                    ),
            )
            .when(matches!(self.operation, Operation::Add), |body| {
                body.child(self.render_creation(cx))
            })
            .when(matches!(self.operation, Operation::Extract | Operation::Copy | Operation::Move), |body| {
                body.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(tr("Destination folder")))
                        .child(
                            h_flex().gap_2()
                                .child(
                                    div().flex_1().min_w_0()
                                        .when(matches!(self.operation, Operation::Extract), |field| {
                                            field.child(Input::new(&self.destination_input).id("extract-destination").small())
                                        })
                                        .when(!matches!(self.operation, Operation::Extract), |field| {
                                            field.child(div().text_sm().text_color(cx.theme().muted_foreground).child(destination))
                                        }),
                                )
                                .child(
                                    Button::new("choose-destination").small().outline().flex_none().icon(IconName::FolderOpen)
                                        .label(tr("Choose folder…")).disabled(self.picking)
                                        .on_click(cx.listener(|this, _, window, cx| this.pick(PickTarget::Destination, window, cx))),
                                ),
                        )
                        .when(matches!(self.operation, Operation::Extract), |section| {
                            section.child(div().text_xs().text_color(cx.theme().muted_foreground)
                                    .child(tr("Enter a path or choose a folder. Relative paths start from the current folder.")))
                        })
                        .when_some(stream_name, |section, name| section.child(
                            div().min_w_0().text_sm().text_color(cx.theme().muted_foreground)
                                .child(trf("Extracts one file: %{name}. The source file is kept.", &[("name", name)])),
                        ))
                        .when(matches!(self.operation, Operation::Copy | Operation::Move) && self.sources.len() == 1, |section| {
                            section.child(div().text_sm().font_semibold().pt_2().child(tr("Name at destination")))
                                .child(Input::new(&self.name).id("operation-name").small())
                        }),
                )
            })
            .when(matches!(self.operation, Operation::Extract), |body| {
                body.child(self.archive_settings.render(false, self.archive_format(cx), cx))
            })
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
