use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{IndexPath, Sizable as _, v_flex};
use gpui_kit::component::{
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::model::archive::{ArchiveFormat, ArchiveOptions, CompressionLevel};
use rs7z::service::archive;

pub(super) struct ArchiveSettings {
    password: Entity<InputState>,
    compression: Entity<SelectState<SearchableVec<String>>>,
    tar_compression: Entity<SelectState<SearchableVec<String>>>,
}

const LEVELS: [&str; 5] = [
    "Default",
    "0 — Store only",
    "1 — Fastest",
    "5 — Standard",
    "9 — Maximum",
];

impl ArchiveSettings {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("Optional password")
            }),
            compression: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        LEVELS
                            .iter()
                            .map(|label| (*label).to_owned())
                            .collect::<Vec<_>>(),
                    ),
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
                            .map(|label| (*label).to_owned())
                            .collect::<Vec<_>>(),
                    ),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
        }
    }

    pub(super) fn set_password(&self, password: &str, window: &mut Window, cx: &mut App) {
        self.password.update(cx, |input, cx| {
            input.set_value(password.to_owned(), window, cx)
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
        let compression = if archive::is_tar(format) {
            &self.tar_compression
        } else {
            &self.compression
        };
        options.compression_level =
            compression
                .read(cx)
                .selected_value()
                .and_then(|value| match value.as_str() {
                    "0 — Store only" => Some(CompressionLevel::Store),
                    "1 — Fastest" => Some(CompressionLevel::Fastest),
                    "5 — Standard" => Some(CompressionLevel::Standard),
                    "9 — Maximum" => Some(CompressionLevel::Maximum),
                    _ => None,
                });
        options
    }

    pub(super) fn render(&self, adding: bool, format: ArchiveFormat, _: &App) -> impl IntoElement {
        v_flex()
            .gap_2()
            .when(archive::supports_password(format), |section| {
                section
                    .child(div().text_sm().font_semibold().child(if adding {
                        "Password (AES-256, optional)"
                    } else {
                        "Archive password (required for encrypted entries)"
                    }))
                    .child(Input::new(&self.password).id("zip-password").small())
            })
            .when(adding, |section| {
                section
                    .child(
                        div()
                            .pt_2()
                            .text_sm()
                            .font_semibold()
                            .child("Compression level (optional)"),
                    )
                    .child(
                        Select::new(if archive::is_tar(format) {
                            &self.tar_compression
                        } else {
                            &self.compression
                        })
                        .id("zip-compression")
                        .accessibility_label("Compression level")
                        .small(),
                    )
            })
    }
}
