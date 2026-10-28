//! Real Metal frames complement the mock-renderer UI integration tests.
#![allow(dead_code)]

#[cfg(target_os = "macos")]
// With harness=false, included #[test] functions are omitted but their imports remain.
#[allow(unused_imports)]
mod application {
    include!("../src/ui/mod.rs");

    pub fn check_sidebar_rendering() {
        use gpui_kit::test::TestWindowExt;
        use std::sync::Arc;

        let mut context = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(gpui_kit::assets::AllAssets),
            gpui_kit::platform::current_headless_renderer,
        );
        context.update(|cx| {
            gpui_kit::init(cx);
            rs7z::theme::ThemeFile::parse(include_str!("../themes/apple.toml"))
                .unwrap()
                .apply(cx);
            init_app_actions(cx);
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let paths = [home, PathBuf::from("/")];
        let (handle, browser) = context.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1060.), px(660.)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Browser::new_at(&path, window, cx)),
            )
            .unwrap()
        });
        let initial_bounds = context
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                paths
                    .each_ref()
                    .map(|path| window.find(format!("favorite-{}", path.display())).bounds())
            })
            .unwrap();

        // Inspect every hover/down/up/leave frame, including clicking the selected
        // destination again. No delay between inputs: exercise rapid switching.
        for cycle in 0..4 {
            for target in [0, 1, 1, 0] {
                for phase in ["hover", "down", "up", "leave"] {
                    let (bounds, selected) = context
                        .update_window(handle, |_, window, cx| {
                            let position = if phase == "leave" {
                                point(px(600.), px(400.))
                            } else {
                                initial_bounds[target].center()
                            };
                            let event = match phase {
                                "down" => MouseDownEvent {
                                    position,
                                    button: MouseButton::Left,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                    first_mouse: false,
                                }
                                .to_platform_input(),
                                "up" => MouseUpEvent {
                                    position,
                                    button: MouseButton::Left,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                }
                                .to_platform_input(),
                                _ => MouseMoveEvent {
                                    position,
                                    pressed_button: None,
                                    modifiers: Default::default(),
                                }
                                .to_platform_input(),
                            };
                            window.dispatch_event(event, cx);
                            window.render_frame(cx);
                            if phase == "up" {
                                assert_eq!(browser.read(cx).current_dir, paths[target]);
                            }
                            let bounds = paths.each_ref().map(|path| {
                                window.find(format!("favorite-{}", path.display())).bounds()
                            });
                            assert_eq!(bounds, initial_bounds, "sidebar geometry changed");
                            let selected = paths
                                .each_ref()
                                .map(|path| browser.read(cx).current_dir == *path);
                            (bounds, selected)
                        })
                        .unwrap();
                    let screenshot = context.capture_screenshot(handle).unwrap();
                    let scale = screenshot.width() as f32 / 1060.;
                    for index in 0..2 {
                        let state = format!("cycle {cycle}, target {target}, {phase}, row {index}");
                        let foreground = if selected[index] {
                            [255, 255, 255]
                        } else {
                            [29, 29, 31]
                        };
                        let background = if selected[index] {
                            [0, 102, 204]
                        } else if index == target && phase != "leave" {
                            if phase == "down" {
                                [224, 224, 224]
                            } else {
                                [240, 240, 240]
                            }
                        } else {
                            [245, 245, 247]
                        };
                        let bounds = bounds[index];
                        let sample = screenshot.get_pixel(
                            ((bounds.right().as_f32() - 8.) * scale) as u32,
                            (bounds.center().y.as_f32() * scale) as u32,
                        );
                        assert_eq!(&sample.0[..3], &background, "background: {state}");
                        // Check the icon and label independently, excluding corners.
                        for (left, right, part) in [(8., 24., "icon"), (28., 100., "label")] {
                            let mut pixels = 0;
                            for y in ((bounds.top().as_f32() + 3.) * scale) as u32
                                ..((bounds.bottom().as_f32() - 3.) * scale) as u32
                            {
                                for x in ((bounds.left().as_f32() + left) * scale) as u32
                                    ..((bounds.left().as_f32() + right) * scale) as u32
                                {
                                    let pixel = screenshot.get_pixel(x, y);
                                    if pixel.0[..3].iter().zip(foreground).all(
                                        |(&value, expected)| {
                                            i16::from(value).abs_diff(expected) < 35
                                        },
                                    ) {
                                        pixels += 1;
                                    }
                                }
                            }
                            assert!(pixels > 20, "missing {part}: {state} ({pixels} pixels)");
                        }
                    }
                }
            }
        }
        // Toolbar ghost buttons share the hover palette with sidebar entries.
        for phase in ["hover", "down", "up", "leave"] {
            let bounds = context
                .update_window(handle, |_, window, cx| {
                    let bounds = window.find("refresh").bounds();
                    match phase {
                        "hover" => window.hover("refresh", cx),
                        "down" => {
                            window.dispatch_event(
                                MouseDownEvent {
                                    position: bounds.center(),
                                    button: MouseButton::Left,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                    first_mouse: false,
                                }
                                .to_platform_input(),
                                cx,
                            );
                        }
                        "up" => {
                            window.dispatch_event(
                                MouseUpEvent {
                                    position: bounds.center(),
                                    button: MouseButton::Left,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                }
                                .to_platform_input(),
                                cx,
                            );
                        }
                        _ => window.hover(format!("favorite-{}", paths[0].display()), cx),
                    }
                    window.render_frame(cx);
                    assert_eq!(window.find("refresh").bounds(), bounds);
                    bounds
                })
                .unwrap();
            let screenshot = context.capture_screenshot(handle).unwrap();
            let scale = screenshot.width() as f32 / 1060.;
            let sample = screenshot.get_pixel(
                ((bounds.left().as_f32() + 5.) * scale) as u32,
                (bounds.center().y.as_f32() * scale) as u32,
            );
            let background = match phase {
                "down" => [224, 224, 224],
                "leave" => [245, 245, 247],
                _ => [240, 240, 240],
            };
            assert_eq!(&sample.0[..3], &background, "toolbar {phase}");
            let mut dark_pixels = 0;
            for y in
                (bounds.top().as_f32() * scale) as u32..(bounds.bottom().as_f32() * scale) as u32
            {
                for x in (bounds.left().as_f32() * scale) as u32
                    ..(bounds.right().as_f32() * scale) as u32
                {
                    if screenshot.get_pixel(x, y).0[..3]
                        .iter()
                        .all(|&value| value < 80)
                    {
                        dark_pixels += 1;
                    }
                }
            }
            assert!(dark_pixels > 20, "missing toolbar icon: {phase}");
        }
        println!("sidebar rendering: 64 interaction frames passed");
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    application::check_sidebar_rendering();
}
