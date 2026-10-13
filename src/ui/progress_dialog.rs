use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use rs7z::model::operations::{OperationError, ProgressSnapshot};
use rs7z::service::logging;
use rs7z::service::operations::ProgressHandle;
use std::time::Duration;

pub(super) struct OperationProgressDialog {
    handle: ProgressHandle,
    snapshot: ProgressSnapshot,
    stopping: bool,
    finished: bool,
    output: Option<String>,
    output_view: Option<Entity<TextareaState>>,
    outcome: String,
}

impl OperationProgressDialog {
    pub(super) fn new(handle: ProgressHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
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

    pub(super) fn finish(
        &mut self,
        result: &Result<(), OperationError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh(window, cx);
        self.finished = true;
        self.outcome = match result {
            Ok(()) => "Extraction complete".into(),
            Err(OperationError::Cancelled) => "Cancelled; partial files remain".into(),
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
            self.outcome = "Extraction failed; see output for details".into();
        }
        cx.notify();
    }
}

impl Render for OperationProgressDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                        .aria_label("7-Zip output"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.finished {
                            self.outcome.clone()
                        } else if self.stopping {
                            "Stopping…".into()
                        } else {
                            "Extracting…".into()
                        }),
                )
                .into_any_element();
        }
        let item = if self.stopping {
            "Stopping…".to_owned()
        } else if self.snapshot.item.is_empty() {
            "Preparing…".to_owned()
        } else {
            self.snapshot.item.clone()
        };
        v_flex()
            .gap_3()
            .child(div().text_sm().truncate().child(item))
            .child(
                Progress::new("operation-progress")
                    .value(self.snapshot.percent)
                    .accessibility_label("Operation progress"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{:.0}%", self.snapshot.percent)),
            )
            .into_any_element()
    }
}
