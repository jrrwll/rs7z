use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::tr;
use rs7z::model::archive::ArchiveIndex;
use rs7z::model::operations::{OperationError, ProgressSnapshot};
use rs7z::service::operations::ProgressHandle;
use rs7z::service::{file::format_size, logging};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn format_duration(duration: Duration) -> String {
    format!("{}.{:03}s", duration.as_secs(), duration.subsec_millis())
}

fn format_progress_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        format!("{}h{}m{}s", seconds / 3600, seconds / 60 % 60, seconds % 60)
    } else if seconds >= 60 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn estimate_remaining(elapsed: Duration, percent: f32) -> Option<Duration> {
    if !percent.is_finite() || percent <= 0. {
        return None;
    }
    Duration::try_from_secs_f64(
        elapsed.as_secs_f64() * (100. - f64::from(percent).min(100.)) / f64::from(percent),
    )
    .ok()
}

pub(super) struct TestResult {
    filename: String,
    folders: Option<usize>,
    files: Option<usize>,
    size: Option<u64>,
    elapsed: Duration,
    error: Option<String>,
}

impl TestResult {
    pub(super) fn new(
        path: &Path,
        index: Option<&ArchiveIndex>,
        error: Option<String>,
        elapsed: Duration,
    ) -> Self {
        let (folders, files, size) = index.map_or((None, None, None), |index| {
            let folders = index.entries.iter().filter(|entry| entry.directory).count();
            let files = index.entries.len() - folders;
            let size = index
                .entries
                .iter()
                .filter(|entry| !entry.directory)
                .fold(0u64, |total, entry| total.saturating_add(entry.size));
            (Some(folders), Some(files), Some(size))
        });
        Self {
            filename: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            folders,
            files,
            size,
            elapsed,
            error,
        }
    }

    fn size_label(&self) -> String {
        match self.size {
            Some(size) if size >= 1024 => format!("{size} B ({})", format_size(size)),
            Some(size) => format!("{size} B"),
            None => "--".into(),
        }
    }

    fn text(&self) -> String {
        let mut text = format!(
            "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}",
            tr("Filename"),
            self.filename,
            tr("Folder"),
            self.folders.map_or("--".into(), |value| value.to_string()),
            tr("File"),
            self.files.map_or("--".into(), |value| value.to_string()),
            tr("Size"),
            self.size_label(),
            tr("Elapsed"),
            format_duration(self.elapsed),
        );
        if let Some(error) = &self.error {
            text.push_str(&format!("\n{}\n", tr("Test Failed:")));
            text.push_str(error);
        } else {
            text.push_str(&format!("\n{}", tr("Test OK!")));
        }
        text
    }
}

fn test_result_row(
    id: &'static str,
    label: &'static str,
    value: String,
    state: Entity<OperationProgressDialog>,
    key: &str,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .test_support()
        .w_full()
        .min_w_0()
        .text_left()
        .aria_label(format!("{}: {value}", tr(label)))
        .gap_2()
        .items_start()
        .child(
            div()
                .w(rems(6.))
                .flex_shrink_0()
                .child(format!("{}:", tr(label))),
        )
        .child(
            div()
                .id(format!("{id}-value"))
                .test_support()
                .flex_1()
                .min_w_0()
                .truncate()
                .aria_label(value.clone())
                .child(value),
        )
        .child(
            Button::new(format!("{id}-copy-{key}"))
                .small()
                .ghost()
                .icon(IconName::Copy)
                .tooltip(format!("{} {}", tr("Copy"), tr(label)))
                .accessibility_label(format!("{} {}", tr("Copy"), tr(label)))
                .on_click(move |_, _, cx| {
                    if let Some(result) = &state.read(cx).test_result {
                        let value = match id {
                            "test-result-filename" => result.filename.clone(),
                            "test-result-folders" => result
                                .folders
                                .map_or("--".into(), |count| count.to_string()),
                            "test-result-files" => {
                                result.files.map_or("--".into(), |count| count.to_string())
                            }
                            "test-result-size" => result.size_label(),
                            "test-result-elapsed" => format_duration(result.elapsed),
                            _ => return,
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                    }
                }),
        )
}

pub(super) struct OperationProgressDialog {
    handle: ProgressHandle,
    snapshot: ProgressSnapshot,
    stopping: bool,
    finished: bool,
    output: Option<String>,
    output_view: Option<Entity<TextareaState>>,
    outcome: String,
    test_result: Option<TestResult>,
    test_error_view: Option<Entity<TextareaState>>,
    operation_started: Option<Instant>,
    completed_elapsed: Option<Duration>,
}

impl OperationProgressDialog {
    pub(super) fn new(
        handle: ProgressHandle,
        operation_started: Option<Instant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let output = handle.output();
        let output_view = output.as_ref().map(|text| {
            cx.new(|cx| {
                let mut state = TextareaState::new(window, cx);
                state.set_value(text.clone(), window, cx);
                state
            })
        });
        Self {
            output,
            output_view,
            outcome: String::new(),
            handle,
            snapshot: ProgressSnapshot::default(),
            stopping: false,
            finished: false,
            test_result: None,
            test_error_view: None,
            operation_started,
            completed_elapsed: None,
        }
    }

    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let Ok(finished) = view.update_in(cx, |this, window, cx| {
                    if !this.finished {
                        this.refresh(window, cx);
                        cx.notify();
                    }
                    this.finished
                }) else {
                    break;
                };
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.stopping {
            logging::info("Cancellation requested");
        }
        self.handle.cancel();
        self.stopping = true;
        cx.notify();
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.snapshot = self.handle.snapshot();
        let output = self.handle.output();
        if output != self.output {
            let output_view = self
                .output_view
                .get_or_insert_with(|| cx.new(|cx| TextareaState::new(window, cx)));
            output_view.update(cx, |state, cx| {
                let text = output.clone().unwrap_or_default();
                let end = text.len();
                state.set_value(text, window, cx);
                state.set_selected_range(end..end, cx);
            });
            self.output = output;
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        self.finished
    }

    pub(super) fn has_test_result(&self) -> bool {
        self.test_result.is_some()
    }

    pub(super) fn finish(
        &mut self,
        result: &Result<(), OperationError>,
        test_result: Option<TestResult>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh(window, cx);
        self.finished = true;
        self.completed_elapsed = self.operation_started.map(|started| started.elapsed());
        if let Some(test_result) = test_result {
            self.test_error_view = test_result.error.as_ref().map(|error| {
                cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .default_value(error.clone())
                        .auto_grow(2, 12)
                })
            });
            self.test_result = Some(test_result);
            cx.notify();
            return;
        }
        self.outcome = match result {
            Ok(()) => tr("Extraction complete"),
            Err(OperationError::Cancelled) => tr("Cancelled; partial files remain"),
            Err(OperationError::Failed(error)) => error.clone(),
        };
        if let Err(OperationError::Failed(error)) = result {
            // Dependency and preflight errors can happen before 7zz produces output.
            if let Some(output) = self.output.as_mut() {
                output.push('\n');
                output.push_str(error);
                if let Some(view) = &self.output_view {
                    view.update(cx, |state, cx| state.set_value(output.clone(), window, cx));
                }
            }
            self.outcome = tr("Extraction failed; see output for details");
        }
        cx.notify();
    }
    fn render_timing(&self) -> impl IntoElement {
        v_flex().when_some(self.operation_started, |view, started| {
            let elapsed = self.completed_elapsed.unwrap_or_else(|| started.elapsed());
            let remaining = if self.finished {
                Some(Duration::ZERO)
            } else {
                estimate_remaining(elapsed, self.snapshot.percent)
            }
            .map(format_progress_duration)
            .unwrap_or_else(|| "--".into());
            view.child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w_48().flex_none().child(tr("Elapsed")))
                            .child(
                                div()
                                    .id("operation-progress-elapsed")
                                    .test_support()
                                    .aria_label(format_progress_duration(elapsed))
                                    .child(format_progress_duration(elapsed)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w_48().flex_none().child(tr("Elapsed Remaining")))
                            .child(
                                div()
                                    .id("operation-progress-remaining")
                                    .test_support()
                                    .aria_label(remaining.clone())
                                    .child(remaining),
                            ),
                    ),
            )
        })
    }
}

impl Render for OperationProgressDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(result) = &self.test_result {
            let result_state = cx.entity();
            let result_key = result.filename.clone();
            let status = if result.error.is_some() {
                "Test Failed:"
            } else {
                "Test OK!"
            };
            return v_flex()
                .w_full()
                .gap_2()
                .text_sm()
                .text_left()
                .min_w_0()
                .child(test_result_row(
                    "test-result-filename",
                    "Filename",
                    result.filename.clone(),
                    result_state.clone(),
                    &result_key,
                ))
                .child(test_result_row(
                    "test-result-folders",
                    "Folder",
                    result
                        .folders
                        .map_or("--".into(), |count| count.to_string()),
                    result_state.clone(),
                    &result_key,
                ))
                .child(test_result_row(
                    "test-result-files",
                    "File",
                    result.files.map_or("--".into(), |count| count.to_string()),
                    result_state.clone(),
                    &result_key,
                ))
                .child(test_result_row(
                    "test-result-size",
                    "Size",
                    result.size_label(),
                    result_state.clone(),
                    &result_key,
                ))
                .child(test_result_row(
                    "test-result-elapsed",
                    "Elapsed",
                    format_duration(result.elapsed),
                    result_state.clone(),
                    &result_key,
                ))
                .child(
                    div()
                        .id("test-result-status")
                        .test_support()
                        .mt_2()
                        .w_full()
                        .text_left()
                        .font_semibold()
                        .aria_label(tr(status))
                        .text_color(if result.error.is_some() {
                            cx.theme().danger
                        } else {
                            cx.theme().success
                        })
                        .child(tr(status)),
                )
                .when_some(self.test_error_view.as_ref(), |view, error| {
                    view.child(
                        Textarea::new(error)
                            .readonly(true)
                            .w_full()
                            .h(rems(10.))
                            .accessibility_id("test-error-detail")
                            .aria_label(tr("Test error details")),
                    )
                })
                .child(
                    Button::new(format!("copy-test-result-{result_key}"))
                        .small()
                        .outline()
                        .icon(IconName::Copy)
                        .label(tr("Copy all"))
                        .on_click(move |_, _, cx| {
                            if let Some(result) = &result_state.read(cx).test_result {
                                cx.write_to_clipboard(ClipboardItem::new_string(result.text()));
                            }
                        }),
                )
                .into_any_element();
        }
        if let Some(output_view) = &self.output_view {
            return v_flex()
                .gap_3()
                .min_w_0()
                .child(
                    Textarea::new(output_view)
                        .readonly(true)
                        .w_full()
                        .h(rems(16.))
                        .accessibility_id("archive-command-output")
                        .aria_label(tr("7-Zip output")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.finished {
                            self.outcome.clone()
                        } else if self.stopping {
                            tr("Stopping…")
                        } else {
                            tr("Extracting…")
                        }),
                )
                .child(self.render_timing())
                .into_any_element();
        }
        let item = if self.stopping {
            tr("Stopping…")
        } else if self.snapshot.item.is_empty() {
            tr("Preparing…")
        } else {
            self.snapshot.item.clone()
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(div().text_sm().truncate().child(item))
            .child(
                div().id("progress-width").test_support().w_full().child(
                    Progress::new("operation-progress")
                        .value(self.snapshot.percent)
                        .color(cx.theme().success)
                        .with_size(px(14.))
                        .accessibility_label(tr("Operation progress")),
                ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{:.0}%", self.snapshot.percent)),
            )
            .child(self.render_timing())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{estimate_remaining, format_duration, format_progress_duration};
    use std::time::Duration;

    #[test]
    fn durations_show_seconds_with_three_decimal_places() {
        for (millis, expected) in [
            (0, "0.000s"),
            (1, "0.001s"),
            (59_012, "59.012s"),
            (60_000, "60.000s"),
            (100_123, "100.123s"),
            (3_661_999, "3661.999s"),
        ] {
            assert_eq!(format_duration(Duration::from_millis(millis)), expected);
        }
    }

    #[test]
    fn progress_durations_show_whole_seconds_minutes_and_hours() {
        for (millis, expected) in [
            (0, "0s"),
            (59_999, "59s"),
            (60_000, "1m0s"),
            (100_123, "1m40s"),
            (3_661_999, "1h1m1s"),
        ] {
            assert_eq!(
                format_progress_duration(Duration::from_millis(millis)),
                expected
            );
        }
    }

    #[test]
    fn remaining_time_handles_unknown_progress_and_completion() {
        let elapsed = Duration::from_secs(100);
        assert_eq!(
            estimate_remaining(elapsed, 25.),
            Some(Duration::from_secs(300))
        );
        assert_eq!(estimate_remaining(elapsed, 100.), Some(Duration::ZERO));
        assert_eq!(estimate_remaining(elapsed, 101.), Some(Duration::ZERO));
        for percent in [0., -1., f32::NAN, f32::INFINITY] {
            assert_eq!(estimate_remaining(elapsed, percent), None);
        }
    }
}
