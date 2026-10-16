use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme as _, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputGroup, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::tr;
use rs7z::model::archive::{
    ArchiveFormat, ArchiveOptions, CompressionLevel, SevenZipMethod, SolidBlockSize, TarFormat,
    ZipEncryption,
};
use rs7z::service::{archive, zip::parse_volume_size};

pub(super) struct ArchiveSettings {
    password: Entity<InputState>,
    confirm_password: Entity<InputState>,
    encryption: Entity<SelectState<SearchableVec<String>>>,
    compression: Entity<SelectState<SearchableVec<String>>>,
    tar_compression: Entity<SelectState<SearchableVec<String>>>,
    tar_format: Entity<SelectState<SearchableVec<String>>>,
    seven_zip_method: Entity<SelectState<SearchableVec<String>>>,
    dictionary_size: Entity<SelectState<SearchableVec<String>>>,
    word_size: Entity<SelectState<SearchableVec<String>>>,
    solid_block_size: Entity<SelectState<SearchableVec<String>>>,
}

const LEVELS: [&str; 7] = [
    "Default",
    "0 — Store",
    "1 — Fastest",
    "3 — Fast",
    "5 — Normal",
    "7 — Maximum",
    "9 — Ultra",
];

const SEVEN_ZIP_METHODS: &[&str] = &["LZMA2", "LZMA", "PPMd", "BZip2"];
const DICTIONARY_SIZES: &[&str] = &[
    "16M", "64K", "256K", "1M", "2M", "3M", "4M", "6M", "8M", "12M", "24M", "32M", "48M", "64M",
    "96M", "128M", "192M", "256M", "384M", "512M", "768M", "1024M", "1536M", "2048M", "3072M",
    "3840M",
];
const WORD_SIZES: &[&str] = &[
    "32", "8", "12", "16", "24", "48", "64", "96", "128", "192", "256", "273",
];
const PPMD_WORD_SIZES: &[&str] = &["32", "8", "12", "16", "24"];
const SOLID_BLOCK_SIZES: &[&str] = &[
    "4G",
    "Non-solid",
    "1M",
    "2M",
    "4M",
    "8M",
    "16M",
    "32M",
    "64M",
    "128M",
    "256M",
    "512M",
    "1G",
    "2G",
    "8G",
    "16G",
    "32G",
    "64G",
    "Solid",
];

fn choice(
    items: &[&str],
    window: &mut Window,
    cx: &mut App,
) -> Entity<SelectState<SearchableVec<String>>> {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(
                items
                    .iter()
                    .map(|item| (*item).to_owned())
                    .collect::<Vec<_>>(),
            ),
            Some(IndexPath::new(0)),
            window,
            cx,
        )
    })
}

fn selected_size(select: &Entity<SelectState<SearchableVec<String>>>, cx: &App) -> Option<u64> {
    select
        .read(cx)
        .selected_value()
        .and_then(|value| parse_volume_size(value).expect("size preset must be valid"))
}

impl ArchiveSettings {
    pub(super) fn password_inputs(&self) -> [&Entity<InputState>; 2] {
        [&self.password, &self.confirm_password]
    }

    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr("Optional password"))
            }),
            confirm_password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr("Re-enter password"))
            }),
            encryption: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["ZipCrypt".into(), "AES-256".into()]),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            compression: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(LEVELS.iter().map(|label| tr(label)).collect::<Vec<_>>()),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            // Stream compressors do not all have a store-only mode.
            tar_compression: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        LEVELS
                            .iter()
                            .filter(|label| !label.starts_with('0'))
                            .map(|label| tr(label))
                            .collect::<Vec<_>>(),
                    ),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            tar_format: choice(&["GNU", "POSIX"], window, cx),
            seven_zip_method: choice(SEVEN_ZIP_METHODS, window, cx),
            dictionary_size: choice(DICTIONARY_SIZES, window, cx),
            word_size: choice(WORD_SIZES, window, cx),
            solid_block_size: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        SOLID_BLOCK_SIZES
                            .iter()
                            .map(|value| match *value {
                                "Non-solid" | "Solid" => tr(value),
                                _ => (*value).to_owned(),
                            })
                            .collect::<Vec<_>>(),
                    ),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
        }
    }

    pub(super) fn method_select(&self) -> &Entity<SelectState<SearchableVec<String>>> {
        &self.seven_zip_method
    }

    pub(super) fn compression_select(&self) -> &Entity<SelectState<SearchableVec<String>>> {
        &self.compression
    }

    fn is_store(&self, cx: &App) -> bool {
        self.compression
            .read(cx)
            .selected_value()
            .is_some_and(|value| value == &tr("0 — Store"))
    }

    pub(super) fn sync_method(&self, window: &mut Window, cx: &mut App) {
        let items = if self.selected_method(cx) == SevenZipMethod::Ppmd {
            PPMD_WORD_SIZES
        } else {
            WORD_SIZES
        };
        let selected = self.word_size.read(cx).selected_value().cloned();
        self.word_size.update(cx, |select, cx| {
            select.set_items(
                SearchableVec::new(
                    items
                        .iter()
                        .map(|item| (*item).to_owned())
                        .collect::<Vec<_>>(),
                ),
                window,
                cx,
            );
            let value = selected
                .as_deref()
                .filter(|value| items.contains(value))
                .unwrap_or("32");
            select.set_selected_value(&value.to_owned(), window, cx);
        });
    }

    fn selected_method(&self, cx: &App) -> SevenZipMethod {
        match self
            .seven_zip_method
            .read(cx)
            .selected_value()
            .map(String::as_str)
        {
            Some("LZMA") => SevenZipMethod::Lzma,
            Some("PPMd") => SevenZipMethod::Ppmd,
            Some("BZip2") => SevenZipMethod::Bzip2,
            _ => SevenZipMethod::Lzma2,
        }
    }

    pub(super) fn set_password(&self, password: &str, window: &mut Window, cx: &mut App) {
        self.password.update(cx, |input, cx| {
            input.set_value(password.to_owned(), window, cx)
        });
        self.confirm_password.update(cx, |input, cx| {
            input.set_value(password.to_owned(), window, cx)
        });
    }

    pub(super) fn validate_password(
        &self,
        adding: bool,
        format: ArchiveFormat,
        cx: &App,
    ) -> Result<(), String> {
        self.password_error(adding, format, cx).map_or(Ok(()), Err)
    }

    pub(super) fn password_error(
        &self,
        adding: bool,
        format: ArchiveFormat,
        cx: &App,
    ) -> Option<String> {
        if !adding || !archive::supports_password(format) {
            return None;
        }
        let password = self.password.read(cx).value();
        let confirmation = self.confirm_password.read(cx).value();
        if password.as_str() == confirmation.as_str() {
            None
        } else if confirmation.is_empty() {
            Some(tr("Re-enter the password."))
        } else {
            Some(tr("Passwords do not match."))
        }
    }

    pub(super) fn focus_confirmation(&self, window: &mut Window, cx: &mut App) {
        self.confirm_password.update(cx, |input, cx| {
            input.focus(window, cx);
        });
    }

    pub(super) fn options_for_format(&self, format: ArchiveFormat, cx: &App) -> ArchiveOptions {
        let mut options = ArchiveOptions::default();
        if matches!(format, ArchiveFormat::Stream(_)) {
            return options;
        }
        let password = self.password.read(cx).value().to_string();
        options.password =
            (archive::supports_password(format) && !password.is_empty()).then_some(password);
        if format == ArchiveFormat::Zip {
            options.zip_encryption = if self
                .encryption
                .read(cx)
                .selected_value()
                .is_some_and(|value| value == "AES-256")
            {
                ZipEncryption::Aes256
            } else {
                ZipEncryption::ZipCrypt
            };
        }
        if archive::is_tar(format) {
            options.tar_format = if self
                .tar_format
                .read(cx)
                .selected_value()
                .is_some_and(|value| value == "POSIX")
            {
                TarFormat::Posix
            } else {
                TarFormat::Gnu
            };
        }
        if format == ArchiveFormat::SevenZip && !self.is_store(cx) {
            let method = self.selected_method(cx);
            options.seven_zip.method = Some(method);
            if method != SevenZipMethod::Bzip2 {
                options.seven_zip.dictionary_size = selected_size(&self.dictionary_size, cx);
                options.seven_zip.word_size = self
                    .word_size
                    .read(cx)
                    .selected_value()
                    .and_then(|value| value.parse().ok());
            }
            let solid = self.solid_block_size.read(cx).selected_value().cloned();
            options.seven_zip.solid_block_size = if solid.as_ref() == Some(&tr("Non-solid")) {
                Some(SolidBlockSize::NonSolid)
            } else if solid.as_ref() == Some(&tr("Solid")) {
                Some(SolidBlockSize::Solid)
            } else {
                selected_size(&self.solid_block_size, cx).map(SolidBlockSize::Size)
            };
        }
        if format == ArchiveFormat::Tar {
            return options;
        }
        let compression = if archive::is_tar(format) {
            &self.tar_compression
        } else {
            &self.compression
        };
        options.compression_level = compression.read(cx).selected_value().and_then(|value| {
            LEVELS
                .iter()
                .position(|label| tr(label) == *value)
                .and_then(|index| match index {
                    1 => Some(CompressionLevel::Store),
                    2 => Some(CompressionLevel::Fastest),
                    3 => Some(CompressionLevel::Fast),
                    4 => Some(CompressionLevel::Normal),
                    5 => Some(CompressionLevel::Maximum),
                    6 => Some(CompressionLevel::Ultra),
                    _ => None,
                })
        });
        options
    }

    pub(super) fn render(&self, adding: bool, format: ArchiveFormat, cx: &App) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(self.render_encryption(adding, format, cx))
            .when(adding, |section| {
                section.child(self.render_compression(format, None, cx))
            })
    }

    pub(super) fn render_encryption(
        &self,
        adding: bool,
        format: ArchiveFormat,
        cx: &App,
    ) -> impl IntoElement {
        let password_error = self.password_error(adding, format, cx);
        v_flex()
            .gap_2()
            .when(archive::supports_password(format), |section| {
                section
                    .child(div().text_sm().font_semibold().child(if adding {
                        tr("Password (optional)")
                    } else {
                        tr("Archive password (required for encrypted entries)")
                    }))
                    .child(Input::new(&self.password).id("zip-password").small())
                    .when(adding, |section| {
                        section
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .child(tr("Confirm password")),
                            )
                            .child(
                                InputGroup::new("zip-confirm-password-group")
                                    .small()
                                    .invalid(password_error.is_some())
                                    .input(
                                        Input::new(&self.confirm_password)
                                            .id("zip-confirm-password")
                                            .aria_label(tr("Confirm password")),
                                    ),
                            )
                            .when_some(password_error, |section, error| {
                                section.child(
                                    div()
                                        .id("zip-password-error")
                                        .test_support()
                                        .text_sm()
                                        .text_color(cx.theme().danger)
                                        .child(error),
                                )
                            })
                    })
                    .child(
                        Checkbox::new("zip-show-password")
                            .checked(!self.password.read(cx).presentation().is_masked())
                            .label(tr("Show password"))
                            .on_click({
                                let password = self.password.clone();
                                let confirmation = self.confirm_password.clone();
                                move |checked, window, cx| {
                                    password.update(cx, |input, cx| {
                                        input.set_masked(!checked, window, cx)
                                    });
                                    confirmation.update(cx, |input, cx| {
                                        input.set_masked(!checked, window, cx)
                                    });
                                    window.refresh();
                                }
                            }),
                    )
            })
            .when(adding && format == ArchiveFormat::Zip, |section| {
                section
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(tr("Encryption method")),
                    )
                    .child(
                        Select::new(&self.encryption)
                            .id("zip-encryption")
                            .accessibility_label(tr("Encryption method"))
                            .small(),
                    )
            })
    }

    pub(super) fn render_compression(
        &self,
        format: ArchiveFormat,
        volume_size: Option<AnyElement>,
        cx: &App,
    ) -> impl IntoElement {
        v_flex()
            .gap_2()
            .when(archive::is_tar(format), |section| {
                section
                    .child(div().text_sm().font_semibold().child(tr("TAR format")))
                    .child(
                        Select::new(&self.tar_format)
                            .id("tar-format")
                            .accessibility_label(tr("TAR format"))
                            .small(),
                    )
            })
            .when(format != ArchiveFormat::Tar, |section| {
                section
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(tr("Compression level (optional)")),
                    )
                    .child(
                        Select::new(if archive::is_tar(format) {
                            &self.tar_compression
                        } else {
                            &self.compression
                        })
                        .id("zip-compression")
                        .accessibility_label(tr("Compression level"))
                        .small(),
                    )
            })
            .children(volume_size)
            .when(
                format == ArchiveFormat::SevenZip && !self.is_store(cx),
                |section| {
                    let method = self.selected_method(cx);
                    section
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    v_flex()
                                        .gap_2()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .child(tr("Compression method")),
                                        )
                                        .child(
                                            Select::new(&self.seven_zip_method)
                                                .id("7z-method")
                                                .accessibility_label(tr("Compression method"))
                                                .small(),
                                        ),
                                )
                                .when(method != SevenZipMethod::Bzip2, |row| {
                                    row.child(
                                        v_flex()
                                            .gap_2()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_semibold()
                                                    .child(tr("Dictionary size")),
                                            )
                                            .child(
                                                Select::new(&self.dictionary_size)
                                                    .id("7z-dictionary")
                                                    .accessibility_label(tr("Dictionary size"))
                                                    .small(),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .when(method != SevenZipMethod::Bzip2, |row| {
                                    row.child(
                                        v_flex()
                                            .gap_2()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_semibold()
                                                    .child(tr("Word size")),
                                            )
                                            .child(
                                                Select::new(&self.word_size)
                                                    .id("7z-word")
                                                    .accessibility_label(tr("Word size"))
                                                    .small(),
                                            ),
                                    )
                                })
                                .child(
                                    v_flex()
                                        .gap_2()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .child(tr("Solid block size")),
                                        )
                                        .child(
                                            Select::new(&self.solid_block_size)
                                                .id("7z-solid")
                                                .accessibility_label(tr("Solid block size"))
                                                .small(),
                                        ),
                                ),
                        )
                },
            )
    }
}
