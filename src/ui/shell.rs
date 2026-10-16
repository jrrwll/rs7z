use super::Browser;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rs7z::i18n::trf;

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered_count = self.filtered().len();
        let selected_name = self.selected_item().map(|item| item.name.clone());
        v_flex()
            .size_full()
            .text_color(cx.theme().foreground)
            .bg(cx.theme().background)
            .on_key_down(cx.listener(Self::handle_key))
            .child(self.render_toolbar(cx))
            .child(self.render_path_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.render_sidebar(cx))
                    .child(self.render_files(cx)),
            )
            .child(
                h_flex()
                    .h(px(28.))
                    .px_3()
                    .gap_2()
                    .border_t(px(1.))
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.selection.len() > 0 {
                        trf(
                            "%{selected} of %{total} selected",
                            &[
                                ("selected", self.selection.len().to_string()),
                                ("total", filtered_count.to_string()),
                            ],
                        )
                    } else {
                        trf("%{count} items", &[("count", filtered_count.to_string())])
                    })
                    .when_some(selected_name, |bar, name| bar.child(format!("  {name}"))),
            )
    }
}
