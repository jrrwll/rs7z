use super::Browser;
use gpui_kit::Entity;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    App, AppContext as _, InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, TestAppContext, Window, point, px, size,
};
use rs7z::model::file::{FileItem, FileKind};
use rs7z::model::operations::OperationError;
use rs7z::service::file::format_modified_seconds;
use std::path::PathBuf;
use std::{
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

fn init_test_app(cx: &mut App) {
    gpui_kit::init(cx);
    super::init_app_actions(cx);
}

fn click(window: &mut Window, cx: &mut App, position: Point<Pixels>, button: MouseButton) {
    window.render_frame(cx);
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Default::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseDownEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    window.dispatch_event(
        MouseUpEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

fn select_item(browser: &Entity<Browser>, name: &str, window: &mut Window, cx: &mut App) {
    let index = browser
        .read(cx)
        .filtered()
        .iter()
        .position(|item| item.name == name)
        .unwrap();
    click(
        window,
        cx,
        point(px(300.), px(160. + index as f32 * 32.)),
        MouseButton::Left,
    );
}

fn edit_text(id: &str, value: &str, window: &mut Window, cx: &mut App) {
    window.click(id.to_owned(), cx);
    window.press("secondary-a", cx);
    window.input(value, cx);
}

#[gpui_kit::test]
fn settings_open_from_toolbar_with_language_controls(cx: &mut TestAppContext) {
    use gpui_kit::OwnedMenuItem;

    cx.update(init_test_app);
    let handle = cx.open_window(size(px(820.), px(660.)), |window, cx| {
        let browser = cx.new(|cx| Browser::new(window, cx));
        Root::new(browser, window, cx)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let terminal = window.find("terminal").bounds();
        let refresh = window.find("refresh").bounds();
        let settings = window.find("settings").bounds();
        assert!(window.find("info").bounds().right() < terminal.left());
        assert!(terminal.right() < refresh.left());
        assert!(refresh.right() < settings.left());
        assert!(settings.right() <= px(820.));
        assert_eq!(settings.size.width, px(44.));
        window.activate_window();
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-,"
            } else {
                "ctrl-,"
            },
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("apply-language").is_some());
        assert!(window.try_find("settings-language-choice").is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.click("settings", cx);
        window.render_frame(cx);
        assert!(window.try_find("settings-language-choice").is_some());
        assert!(window.find("external-language-file").visible());
        assert!(window.find("browse-language").visible());
        let builtin = window.within("built-in-language").find("input").bounds();
        let external = window.find("external-language-file").bounds();
        let browse = window.find("browse-language").bounds();
        assert_eq!(builtin.left(), external.left());
        assert_eq!(builtin.right(), browse.right());
        assert!(builtin.bottom() < external.top());
        window.within("built-in-language").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("built-in-language").value(),
            Some("Simplified Chinese")
        );
        edit_text(
            "external-language-file",
            "/tmp/custom-language.toml",
            window,
            cx,
        );
        window.within("language-source").click(1usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within("language-source").find(1usize).selected(),
            Some(true)
        );
        assert!(window.find("built-in-language").visible());
        assert!(window.find("browse-language").visible());
        window.within("language-source").click(0usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within("language-source").find(0usize).selected(),
            Some(true)
        );
        assert_eq!(
            window.find("external-language-file").value(),
            Some("/tmp/custom-language.toml")
        );
        assert!(window.find("browse-language").visible());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, _| window.activate_window())
        .unwrap();
    cx.update(|cx| {
        let menus = cx.get_menus().unwrap();
        let menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "rs7z")
            .unwrap();
        let OwnedMenuItem::Action { action, .. } = &menu.items[0] else {
            panic!("Missing native Settings action");
        };
        assert!(action.as_any().is::<super::OpenSettings>());
        cx.dispatch_action(action.as_ref());
    });
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("settings-language-choice").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn zip_creation_requires_matching_passwords_and_can_reveal_them(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(820.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        window.click("add", cx);
        assert!(window.try_find("zip-encryption").is_some());
        assert!(window.try_find("zip-confirm-password").is_some());
        let destination = window.find("add-destination").bounds();
        let choose = window.find("choose-destination").bounds();
        assert_eq!(destination.center().y, choose.center().y);
        assert!(destination.right() < choose.left());
        let name = window.find("operation-name").bounds();
        let format = window.find("archive-format").bounds();
        assert_eq!(name.top(), format.top());
        assert!(name.right() < format.left());
        let compression = window.find("zip-compression").bounds();
        let password = window.find("zip-password").bounds();
        let volume = window.find("archive-volume-size-group").bounds();
        assert_eq!(compression.top(), password.top());
        assert_eq!(compression.left(), volume.left());
        assert!(compression.right() < password.left());
        assert!(volume.right() < password.left());
        assert!(password.right() <= choose.right());
        assert!(choose.right() < px(820.));
        let dialog = window.within("dialog").find(0usize).bounds();
        assert!(dialog.size.height > px(600.));
        assert!(dialog.top() >= px(0.) && dialog.bottom() <= px(660.));
        window.scroll("dialog", ScrollDelta::Lines(point(0., -12.)), cx);
        edit_text("zip-password", "s", window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("zip-show-password", cx);
        assert_eq!(window.find("zip-password").value(), Some("s"));
        window.click("zip-show-password", cx);
        assert!(window.find("zip-password-error").visible());
        window.input("ecret", cx);
        edit_text("zip-confirm-password", "different", window, cx);
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("confirm-operation").is_some());
        assert!(!browser.read(cx).busy);
        assert!(!directory.join("item.zip").exists());

        window.click("zip-show-password", cx);
        assert_eq!(window.find("zip-show-password").checked(), Some(true));
        assert_eq!(window.find("zip-password").value(), Some("secret"));
        assert_eq!(
            window.find("zip-confirm-password").value(),
            Some("different")
        );
        window.click("zip-show-password", cx);
        assert_eq!(window.find("zip-password").value(), None);
        assert_eq!(window.find("zip-confirm-password").value(), None);
        edit_text("zip-confirm-password", "sec", window, cx);
        assert!(window.find("zip-password-error").visible());
        window.input("ret", cx);
        assert!(window.try_find("zip-password-error").is_none());
        edit_text("zip-password", "changed", window, cx);
        assert!(window.find("zip-password-error").visible());
        edit_text("zip-password", "secret", window, cx);
        assert!(window.try_find("zip-password-error").is_none());
        window.within("zip-encryption").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(directory.join("item.zip").exists());
    let mut archive =
        zip::ZipArchive::new(fs::File::open(directory.join("item.zip")).unwrap()).unwrap();
    assert!(
        archive
            .get_aes_verification_key_and_salt(0)
            .unwrap()
            .is_some()
    );
    assert!(archive.by_name_decrypt("item.txt", b"secret").is_ok());
}

#[gpui_kit::test]
fn archive_add_revalidates_passwords_as_they_change(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temporary.path())
        .unwrap()
        .join("existing.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("docs/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.finish().unwrap();

    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "docs", window, cx);
        window.click("add", cx);
        assert!(window.try_find("confirm-archive-operation").is_some());
        window.scroll("dialog", ScrollDelta::Lines(point(0., -12.)), cx);
        edit_text("zip-password", "abc", window, cx);
        assert!(window.find("zip-password-error").visible());
        window.click("confirm-archive-operation", cx);
        assert!(window.try_find("confirm-archive-operation").is_some());
        edit_text("zip-confirm-password", "ab", window, cx);
        assert!(window.find("zip-password-error").visible());
        window.input("c", cx);
        assert!(window.try_find("zip-password-error").is_none());
        edit_text("zip-password", "abcd", window, cx);
        assert!(window.find("zip-password-error").visible());
        window.click("zip-password", cx);
        window.press("secondary-a", cx);
        window.press("backspace", cx);
        window.click("zip-confirm-password", cx);
        window.press("secondary-a", cx);
        window.press("backspace", cx);
        assert!(window.try_find("zip-password-error").is_none());
    })
    .unwrap();
}

fn assert_menu_shortcuts(
    handle: gpui_kit::AnyWindowHandle,
    cx: &TestAppContext,
    selectors: &[&'static str],
) {
    let mut visual = gpui_kit::VisualTestContext::from_window(handle, cx);
    let mut right_edge = None;
    for (ix, selector) in selectors.iter().enumerate() {
        let hint = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("context menu should show {selector}"));
        let row = visual.update(|window, _| window.within("popup-menu").find(ix).bounds());
        assert!(hint.size.width > px(0.) && hint.size.height > px(0.));
        assert!(
            hint.left() > row.center().x,
            "{selector} should be on the right"
        );
        assert!(row.contains(&hint.origin) && row.contains(&hint.bottom_right()));
        if let Some(right_edge) = right_edge {
            assert_eq!(hint.right(), right_edge, "shortcut hints should align");
        }
        right_edge = Some(hint.right());
    }
}

#[gpui_kit::test]
fn file_operation_shortcuts_use_selection_rules_and_delete_confirmation(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"contents").unwrap();
    let path = directory.join("contents.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("inside.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"inside").unwrap();
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for (name, shortcut, field, value) in [
        (
            "item.txt",
            "alt-a",
            "operation-name",
            Some("item.zip".to_owned()),
        ),
        (
            "item.txt",
            "alt-c",
            "operation-name",
            Some("item.txt copy".to_owned()),
        ),
        (
            "item.txt",
            "alt-m",
            "operation-name",
            Some("item.txt".to_owned()),
        ),
        (
            "contents.zip",
            "alt-e",
            "extract-destination",
            Some(directory.join("contents").display().to_string()),
        ),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, name, window, cx);
            window.press(shortcut, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("confirm-operation").is_some(), "{shortcut}");
            assert_eq!(window.find(field).value(), value.as_deref());
            assert_eq!(browser.read(cx).selection.len(), 1);
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "contents.zip", window, cx);
        window.press("alt-t", cx);
        assert!(window.try_find("confirm-operation").is_none());
        assert!(window.try_find("choose-source-file").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("test-result-status").label(), Some("Test OK!"));
        assert!(window.try_find("choose-source-file").is_none());
        window.within("dialog").click("close", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "contents.zip", window, cx);
        window.click("test", cx);
        assert!(window.try_find("confirm-operation").is_none());
        assert!(window.try_find("choose-source-file").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("test-result-status").label(), Some("Test OK!"));
        window.within("dialog").click("close", cx);
    })
    .unwrap();
    cx.run_until_parked();
    for (shortcut, control) in [("alt-r", "confirm-rename"), ("alt-i", "copy-properties")] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, "item.txt", window, cx);
            window.press(shortcut, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(control).is_some(), "{shortcut}");
            if shortcut == "alt-r" {
                assert_eq!(window.find("rename-name").value(), Some("item.txt"));
                assert_eq!(window.find("rename-name").focused(), Some(true));
            } else {
                window.click("copy-properties", cx);
                assert!(
                    cx.read_from_clipboard()
                        .unwrap()
                        .text()
                        .unwrap()
                        .contains("Name: item.txt")
                );
            }
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "contents.zip", window, cx);
        for key in ["secondary-c", "secondary-e", "alt-x"] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none(), "{key}");
        }
        select_item(&browser, "item.txt", window, cx);
        for key in ["alt-e", "alt-t"] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none(), "{key}");
        }
        window.press("secondary-a", cx);
        assert_eq!(browser.read(cx).selection.len(), 2);
        assert!(window.try_find("dialog").is_none());
        for key in ["alt-e", "alt-t", "alt-r", "alt-i"] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none(), "{key}");
        }
        window.press("delete", cx);
        assert!(window.try_find("dialog").is_some());
        assert!(path.exists() && directory.join("item.txt").exists());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        window.press("alt-d", cx);
        assert!(window.try_find("dialog").is_some());
        assert!(directory.join("item.txt").exists());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        browser.update(cx, |browser, _| browser.busy = true);
        for key in [
            "alt-a", "alt-e", "alt-t", "alt-c", "alt-m", "alt-d", "alt-r", "alt-i", "delete",
        ] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none());
        }
        window.press("secondary-a", cx);
        assert_eq!(browser.read(cx).selection.len(), 1);
        browser.update(cx, |browser, _| browser.busy = false);
        window.press("escape", cx);
        for key in [
            "alt-e", "alt-t", "alt-c", "alt-m", "alt-d", "alt-r", "delete",
        ] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none());
        }
        window.press("alt-a", cx);
        assert_eq!(window.find("operation-name").value(), Some("archive.zip"));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).selection.len(), 0);
        window.press("alt-i", cx);
        window.click("copy-properties", cx);
        let details = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(details.contains(&format!("Path: {}", directory.display())));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        // A Mac's Delete key sends Backspace; extended keyboards send Delete.
        window.press(
            if cfg!(target_os = "macos") {
                "backspace"
            } else {
                "delete"
            },
            cx,
        );
        assert!(window.try_find("dialog").is_some());
        assert!(directory.join("item.txt").exists());
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!directory.join("item.txt").exists());
    assert!(path.exists());
    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| browser.navigate(&path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "inside.txt", window, cx);
        for key in ["alt-a", "alt-e", "alt-t"] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none());
        }
    })
    .unwrap();
    for (shortcut, control) in [
        ("alt-c", "confirm-archive-operation"),
        ("alt-m", "confirm-archive-operation"),
        ("alt-r", "confirm-rename"),
        ("alt-i", "copy-properties"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.press(shortcut, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(control).is_some(), "{shortcut}");
            match shortcut {
                "alt-c" | "alt-m" => assert_eq!(
                    window.find("archive-system-path").value(),
                    Some(directory.to_str().unwrap())
                ),
                "alt-r" => assert_eq!(window.find("rename-name").value(), Some("inside.txt")),
                "alt-i" => {
                    window.click("copy-properties", cx);
                    assert!(
                        cx.read_from_clipboard()
                            .unwrap()
                            .text()
                            .unwrap()
                            .contains("Name: inside.txt")
                    );
                }
                _ => unreachable!(),
            }
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn file_shortcuts_preserve_text_editing_in_inputs_and_dialogs(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = std::fs::canonicalize(temporary.path()).unwrap();
    let file = directory.join("keep.txt");
    std::fs::write(&file, b"keep").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "keep.txt", window, cx);
        let input = browser.read(cx).path_input.clone();
        window.click(("input", input.entity_id()), cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(directory.to_str().unwrap())
        );
        window.press("delete", cx);
        assert!(input.read(cx).value().is_empty());
        window.press("secondary-e", cx);
        assert!(window.try_find("dialog").is_none());
        for key in [
            "alt-a", "alt-e", "alt-t", "alt-c", "alt-m", "alt-d", "alt-r", "alt-i",
        ] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none(), "{key}");
        }
        assert!(file.exists());
        select_item(&browser, "keep.txt", window, cx);
        window.press("alt-a", cx);
        window.click("operation-name", cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("keep.zip")
        );
        window.press("backspace", cx);
        assert_eq!(window.find("operation-name").value(), Some(""));
        window.input("new.zip", cx);
        window.press("secondary-e", cx);
        for key in [
            "alt-a", "alt-e", "alt-t", "alt-c", "alt-m", "alt-d", "alt-r", "alt-i",
        ] {
            window.press(key, cx);
            assert!(window.try_find("extract-destination").is_none(), "{key}");
            assert!(window.try_find("confirm-operation").is_some(), "{key}");
        }
        assert!(window.try_find("extract-destination").is_none());
        assert!(window.try_find("confirm-operation").is_some());
        assert!(file.exists());
    })
    .unwrap();
}

#[gpui_kit::test]
fn add_accepts_typed_save_folders_and_keeps_archive_name_separate(cx: &mut TestAppContext) {
    use rs7z::service::archive;
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"contents").unwrap();
    let absolute = directory.join("absolute folder 中文");
    let relative = directory.join("relative folder 中文");
    fs::create_dir(&absolute).unwrap();
    fs::create_dir(&relative).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for (folder, expected, name) in [
        (absolute.display().to_string(), absolute, "typed.zip"),
        ("relative folder 中文".into(), relative, "typed.7z"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            select_item(&browser, "item.txt", window, cx);
            window.click("add", cx);
            assert_eq!(
                window.find("add-destination").value(),
                Some(directory.to_str().unwrap())
            );
            assert_eq!(window.find("operation-name").value(), Some("item.zip"));
            for invalid in ["", "missing folder", "item.txt"] {
                edit_text("add-destination", invalid, window, cx);
                if invalid.is_empty() {
                    window.press("backspace", cx);
                }
                assert_eq!(window.find("add-destination").value(), Some(invalid));
                window.click("confirm-operation", cx);
                assert!(window.try_find("confirm-operation").is_some());
                assert!(!browser.read(cx).busy);
                assert_eq!(window.find("add-destination").value(), Some(invalid));
            }
            edit_text("add-destination", &folder, window, cx);
            if name.ends_with("7z") {
                window.within("archive-format").click("input", cx);
                window.press("down", cx);
                window.press("enter", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("add-destination").value(),
                Some(folder.as_str())
            );
            edit_text("operation-name", name, window, cx);
            window.click("confirm-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            assert!(window.try_find("dialog").is_none());
        })
        .unwrap();
        let index = archive::read(&expected.join(name)).unwrap();
        assert!(
            index
                .entries
                .iter()
                .any(|entry| entry.name == "item.txt" && entry.size == 8)
        );
        assert!(!directory.join(name).exists());
    }
}

#[gpui_kit::test]
fn quit_shortcut_is_global_in_lists_inputs_and_dialogs(cx: &mut TestAppContext) {
    use gpui_kit::OwnedMenuItem;
    use std::{cell::Cell, rc::Rc};

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let requests = Rc::new(Cell::new(0));
    let observed = requests.clone();
    cx.update(|cx| {
        // TestPlatform::quit is a no-op; observe the action and propagate to the real handler.
        cx.on_action(move |_: &super::Quit, cx| {
            observed.set(observed.get() + 1);
            cx.propagate();
        });
    });
    let temporary = tempfile::tempdir().unwrap();
    let directory = std::fs::canonicalize(temporary.path()).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    let shortcut = if cfg!(target_os = "macos") {
        "cmd-q"
    } else {
        "ctrl-q"
    };
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let focus = browser.read(cx).focus.clone();
        window.focus(&focus, cx);
        window.press(shortcut, cx);
        assert_eq!(requests.get(), 1);
        let path_input = browser.read(cx).path_input.clone();
        path_input.update(cx, |input, cx| input.focus(window, cx));
        window.press(shortcut, cx);
        assert_eq!(requests.get(), 2);
        assert_eq!(
            path_input.read(cx).value().as_str(),
            directory.to_str().unwrap()
        );
        window.click("add", cx);
        window.click("add-destination", cx);
        window.press(shortcut, cx);
        assert_eq!(requests.get(), 3);
        assert_eq!(
            window.find("add-destination").value(),
            Some(directory.to_str().unwrap())
        );
        let menus = cx.get_menus().unwrap();
        let menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "rs7z")
            .unwrap();
        let OwnedMenuItem::Action { name, action, .. } = &menu.items[0] else {
            panic!("Missing native Settings action");
        };
        assert_eq!(name, "Settings…");
        assert!(action.as_any().is::<super::OpenSettings>());
        assert!(matches!(menu.items[1], OwnedMenuItem::Separator));
        let OwnedMenuItem::Action {
            action,
            name,
            disabled,
            ..
        } = &menu.items[2]
        else {
            panic!("Missing native Quit action");
        };
        assert_eq!(name, "Quit rs7z");
        assert!(!disabled);
        assert!(action.as_any().is::<super::Quit>());
        cx.dispatch_action(action.as_ref());
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(requests.get(), 4);
}

#[gpui_kit::test]
fn extract_accepts_typed_paths_and_shows_failures_in_a_dialog(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    let archive = directory.join("contents.zip");
    let mut zip = ZipWriter::new(File::create(&archive).unwrap());
    zip.start_file("file.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"extracted contents").unwrap();
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    let default = archive.with_extension("").display().to_string();
    for input in [
        directory.join("absolute/new folder").display().to_string(),
        "relative/new folder".into(),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, "contents.zip", window, cx);
            window.click("extract", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("extract-destination").value(),
                Some(default.as_str())
            );
            assert_eq!(window.find("extract-destination").focused(), Some(true));
            // Typing without clicking proves the initial caret is at the end.
            window.input(" 中文", cx);
            assert_eq!(
                window.find("extract-destination").value(),
                Some(format!("{default} 中文").as_str())
            );
            edit_text("extract-destination", &input, window, cx);
            window.click("confirm-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            assert!(window.try_find("dialog").is_none());
        })
        .unwrap();
        let target = directory.join(&input);
        assert_eq!(
            fs::read(target.join("file.txt")).unwrap(),
            b"extracted contents"
        );
    }
    // Reusing a nonempty destination produces a real service error after progress closes.
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "contents.zip", window, cx);
        window.click("extract", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        edit_text("extract-destination", "relative/new folder", window, cx);
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(
            browser
                .read(cx)
                .error
                .as_deref()
                .unwrap()
                .contains("not empty")
        );
        assert!(window.try_find("dialog").is_some());
        assert!(window.try_find("cancel-progress").is_none());
        assert!(window.try_find("dismiss-error").is_none());
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(browser.read(cx).error.is_none());
        assert!(window.try_find("dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn rename_initial_cursor_respects_file_extension_and_directory_names(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for (name, kind, expected) in [
        ("archive.tar.zip", FileKind::Archive, "archive.tar_new.zip"),
        ("报告.final.txt", FileKind::Other, "报告.final_new.txt"),
        ("README", FileKind::Other, "README_new"),
        ("资料.v1", FileKind::Directory, "资料.v1_new"),
        ("folder", FileKind::Directory, "folder_new"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                let path = PathBuf::from("/tmp").join(name);
                browser.files = vec![FileItem {
                    path: path.clone(),
                    name: name.into(),
                    kind,
                    size: 0,
                    modified: None,
                    created: None,
                    accessed: None,
                    archive: None,
                }];
                browser.selection.context_click(path);
                browser.begin_rename(window, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("rename-name").focused(), Some(true));
            window.input("_new", cx);
            assert_eq!(window.find("rename-name").value(), Some(expected));
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn filesystem_rename_and_edit_archive_name(cx: &mut TestAppContext) {
    use std::fs::{self, File};
    use zip::ZipArchive;
    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("first.txt"), b"first").unwrap();
    fs::write(directory.join("second.txt"), b"second").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "first.txt", window, cx);
        window.click("rename", cx);
        assert_eq!(window.find("rename-name").value(), Some("first.txt"));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("rename-name").focused(), Some(true));
        window.input(" new", cx);
        assert_eq!(window.find("rename-name").value(), Some("first new.txt"));
        window.press("secondary-a", cx);
        window.input("draft.txt", cx);
        assert_eq!(window.find("rename-name").value(), Some("draft.txt"));
    })
    .unwrap();
    for invalid in ["../escape", "second.txt"] {
        cx.update_window(handle.into(), |_, window, cx| {
            edit_text("rename-name", invalid, window, cx);
            window.click("confirm-rename", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("confirm-rename").is_some());
        })
        .unwrap();
        assert!(directory.join("first.txt").exists());
        assert_eq!(fs::read(directory.join("second.txt")).unwrap(), b"second");
    }
    cx.update_window(handle.into(), |_, window, cx| {
        edit_text("rename-name", "renamed.txt", window, cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!directory.join("first.txt").exists());
    assert_eq!(fs::read(directory.join("renamed.txt")).unwrap(), b"first");
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "renamed.txt", window, cx);
        window.press("secondary-a", cx);
        window.click("rename", cx);
        assert!(window.try_find("confirm-rename").is_none());
        window.click("add", cx);
        assert_eq!(window.find("operation-name").value(), Some("archive.zip"));
        edit_text("operation-name", "../escape.zip", window, cx);
        window.click("confirm-operation", cx);
        assert!(window.try_find("confirm-operation").is_some());
        edit_text("operation-name", "my archive.zip", window, cx);
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!directory.join("archive.zip").exists());
    let mut zip = ZipArchive::new(File::open(directory.join("my archive.zip")).unwrap()).unwrap();
    assert!(zip.by_name("renamed.txt").is_ok());
    assert!(zip.by_name("second.txt").is_ok());
}

#[gpui_kit::test]
fn archive_rename_and_add_selection_rules(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temporary.path())
        .unwrap()
        .join("rename.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    for name in ["docs/item.txt", "other.txt"] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contents").unwrap();
    }
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "docs", window, cx);
        assert!(browser.read(cx).can_add());
        window.click("add", cx);
        assert!(window.try_find("confirm-archive-operation").is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "other.txt", window, cx);
        assert!(!browser.read(cx).can_add());
        window.click("add", cx);
        assert!(window.try_find("confirm-archive-operation").is_none());
        click(window, cx, point(px(300.), px(192.)), MouseButton::Right);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("confirm-archive-operation").is_none());
        window.within("popup-menu").click(4usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("rename-name").value(), Some("other.txt"));
        assert_eq!(window.find("rename-name").focused(), Some(true));
        window.press("secondary-a", cx);
        window.input("renamed.txt", cx);
        window.click("confirm-rename", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "docs", window, cx);
        window.click("rename", cx);
        edit_text("rename-name", "new docs", window, cx);
        window.click("confirm-rename", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    assert!(zip.by_name("renamed.txt").is_ok());
    assert!(zip.by_name("other.txt").is_err());
    assert!(zip.by_name("new docs/item.txt").is_ok());
    assert!(zip.by_name("docs/item.txt").is_err());
}

#[gpui_kit::test]
fn file_list_clear_and_context_menu(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let first = PathBuf::from("/tmp/rs7z-ui-first");
    let second = PathBuf::from("/tmp/rs7z-ui-second");
    let archive = PathBuf::from("/tmp/rs7z-ui-archive.zip");
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.files = vec![
                FileItem {
                    path: first.clone(),
                    name: "First".into(),
                    kind: FileKind::Directory,
                    size: 0,
                    modified: None,
                    created: None,
                    accessed: None,
                    archive: None,
                },
                FileItem {
                    path: second.clone(),
                    name: "Second".into(),
                    kind: FileKind::Other,
                    size: 0,
                    modified: None,
                    created: None,
                    accessed: None,
                    archive: None,
                },
                FileItem {
                    path: archive.clone(),
                    name: "Archive.zip".into(),
                    kind: FileKind::Archive,
                    size: 0,
                    modified: None,
                    created: None,
                    accessed: None,
                    archive: None,
                },
            ];
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        assert_eq!(browser.read(cx).selection.only(), Some(&first));
        assert!(!browser.read(cx).can_extract_or_test());

        click(window, cx, point(px(300.), px(300.)), MouseButton::Left);
        assert_eq!(browser.read(cx).selection.len(), 0);
        assert!(!browser.read(cx).can_extract_or_test());

        window.click("copy", cx);
        window.click("move", cx);
        assert!(window.try_find("confirm-operation").is_none());
        click(window, cx, point(px(300.), px(300.)), MouseButton::Right);
    })
    .unwrap();
    cx.run_until_parked();
    for index in [1usize, 2, 3, 4, 5, 6] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("popup-menu").is_some());
            assert_eq!(browser.read(cx).selection.len(), 0);
            window.within("popup-menu").click(index, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("confirm-operation").is_none());
            assert!(window.try_find("confirm-rename").is_none());
            assert!(window.try_find("dialog").is_none());
            assert!(window.try_find("popup-menu").is_some());
        })
        .unwrap();
    }
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(
            window.within("popup-menu").find(0usize).label(),
            Some("Add to archive…")
        );
        assert_eq!(
            window.within("popup-menu").find(8usize).label(),
            Some("Refresh")
        );
        window.within("popup-menu").click(0usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("confirm-operation").is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(192.)), MouseButton::Right);
        assert_eq!(browser.read(cx).selection.only(), Some(&second));
        assert!(!browser.read(cx).can_extract_or_test());
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("popup-menu").is_some());
        let menu = window.within("popup-menu");
        for (index, label) in [
            "Add to archive…",
            "Extract archive…",
            "Test archive…",
            "Copy…",
            "Move…",
            "Delete",
            "Rename…",
            "Info…",
            "Refresh",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(menu.find(index).label(), Some(label));
        }
        assert_eq!(menu.find(9usize).label(), Some("CRC"));
        assert_eq!(menu.find(10usize).label(), None);
        assert_eq!(menu.find(11usize).label(), Some("Open in terminal"));
    })
    .unwrap();
    assert_menu_shortcuts(
        handle.into(),
        cx,
        &[
            "kbd:alt-a",
            "kbd:alt-e",
            "kbd:alt-t",
            "kbd:alt-c",
            "kbd:alt-m",
            "kbd:alt-d",
            "kbd:alt-r",
            "kbd:alt-i",
        ],
    );

    cx.update_window(handle.into(), |_, window, cx| window.press("escape", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(160.)), MouseButton::Right);
        assert_eq!(browser.read(cx).selection.only(), Some(&first));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        assert_eq!(menu.find(9usize).label(), None);
        assert_eq!(menu.find(10usize).label(), Some("Open in terminal"));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(224.)), MouseButton::Left);
        assert_eq!(browser.read(cx).selection.only(), Some(&archive));
        assert!(browser.read(cx).can_extract_or_test());
    })
    .unwrap();
    cx.update(|cx| {
        browser.update(cx, |browser, cx| {
            browser.selection.context_click(second.clone());
            browser.selection.click(
                &[first.clone(), second.clone(), archive.clone()],
                first.clone(),
                true,
                false,
            );
            cx.notify();
        });
    });
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(192.)), MouseButton::Right);
        assert_eq!(browser.read(cx).selection.len(), 2);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).can_extract_or_test());
        window.within("popup-menu").click(6usize, cx);
        assert!(window.try_find("confirm-rename").is_none());
        window.within("popup-menu").click(8usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(browser.read(cx).selection.len(), 0));
}

#[gpui_kit::test]
async fn crc_submenu_shows_md5_and_all_results(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    for (index, names) in [
        (6usize, vec!["MD5"]),
        (
            7,
            vec![
                "CRC32", "CRC64", "SHA256", "SHA1", "BLAKE2sp", "MD5", "XXH64", "SHA384", "SHA512",
                "SHA3-256",
            ],
        ),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, "item.txt", window, cx);
            click(window, cx, point(px(300.), px(160.)), MouseButton::Right);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.within("popup-menu").hover(9usize, cx);
        })
        .unwrap();
        cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
            window.try_find("submenu").is_some()
        })
        .await;
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(
                window.within("submenu").find(index).label(),
                Some(if index == 6 { "MD5" } else { "*" })
            );
            window.within("submenu").click(index, cx);
        })
        .unwrap();
        cx.wait_for(handle.into(), Duration::from_secs(5), |window, _| {
            window.try_find("copy-checksums").is_some()
        })
        .await;
        cx.update_window(handle.into(), |_, window, cx| {
            let first = names[0];
            let name_cell = window.find(format!("checksum-name-{first}"));
            let value_cell = window.find(format!("checksum-value-{first}"));
            assert_eq!(name_cell.label(), Some(first));
            assert!(name_cell.bounds().right() < value_cell.bounds().left());
            window.click(format!("copy-checksum-{first}"), cx);
            let individual = cx.read_from_clipboard().unwrap().text().unwrap();
            assert_eq!(
                window.find(format!("checksum-value-{first}")).label(),
                Some(individual.as_str())
            );

            window.click("copy-checksums", cx);
            let result = cx.read_from_clipboard().unwrap().text().unwrap();
            let lines: Vec<_> = result.lines().skip(1).collect();
            assert_eq!(lines.len(), names.len());
            for (line, name) in lines.iter().zip(&names) {
                assert!(line.starts_with(&format!("{name}: ")), "{line}");
                let name_cell = window.find(format!("checksum-name-{name}"));
                let value_cell = window.find(format!("checksum-value-{name}"));
                assert_eq!(
                    name_cell.bounds().left(),
                    window
                        .find(format!("checksum-name-{first}"))
                        .bounds()
                        .left()
                );
                assert_eq!(
                    value_cell.bounds().left(),
                    window
                        .find(format!("checksum-value-{first}"))
                        .bounds()
                        .left()
                );
            }
            if names.len() == 10 {
                assert!(
                    window.find("checksum-value-SHA512").bounds().size.height
                        > window.find("checksum-value-MD5").bounds().size.height
                );
            }
            assert_eq!(lines[0].split_once(": ").unwrap().1, individual);
            window.press("escape", cx);
            assert!(window.try_find("copy-checksums").is_none());
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn file_columns_resize_without_changing_selection(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let folder = fs::canonicalize(temporary.path()).unwrap();
    let path = folder.join("item.txt");
    fs::write(&path, b"contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&folder, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("column-created").is_some());
        assert!(window.try_find("column-accessed").is_some());
        assert!(window.try_find("column-compressed-size").is_none());
        let before = window.find("column-name").bounds().size.width;
        let start = window.find("resize-name").bounds().center();
        let end = point(start.x + px(48.), start.y);
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: start,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: end,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find("column-name").bounds().size.width > before + px(30.));
        assert_eq!(
            window.find("column-size").bounds().left(),
            window
                .find(format!("file-cell-{}-size", path.display()))
                .bounds()
                .left()
        );
        assert_eq!(browser.read(cx).selection.len(), 0);
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        assert_eq!(browser.read(cx).selection.only(), Some(&path));
    })
    .unwrap();
}

#[gpui_kit::test]
fn archive_metadata_columns_scroll_with_rows(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    let path = fs::canonicalize("tests/fixtures/zipx-deflate64.zipx").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(browser.read(cx).archive.is_some());
        for column in ["compressed-size", "encrypted", "comment", "crc"] {
            assert!(
                window.try_find(format!("column-{column}")).is_some(),
                "{column}"
            );
        }
        let cell = |column| format!("file-cell-{}-{column}", path.join("item.txt").display());
        assert_eq!(
            window.find(cell("compressed-size")).label(),
            Some("61 bytes")
        );
        assert_eq!(window.find(cell("encrypted")).label(), Some("No"));
        assert_eq!(window.find(cell("comment")).label(), Some("--"));
        assert_eq!(window.find(cell("crc")).label(), Some("89749A40"));
        let start = window.find("column-name").bounds().left();
        window.scroll(
            "file-horizontal-scroll",
            ScrollDelta::Pixels(point(px(-400.), px(0.))),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find("column-name").bounds().left() < start);
        assert!(
            window
                .find(format!("file-{}", path.join("item.txt").display()))
                .visible()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn current_directory_info_and_terminal_from_toolbar_and_context_menu(cx: &mut TestAppContext) {
    use rs7z::model::config::TerminalConfig;
    use std::fs;
    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            // A harmless stand-in records which directory the terminal receives.
            view.terminal =
                TerminalConfig::Command(vec!["/usr/bin/touch".into(), "$1/terminal-marker".into()]);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for menu in [false, true] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).selection.len(), 0);
            assert!(browser.read(cx).can_show_info());
            assert!(browser.read(cx).can_open_terminal());
            assert_eq!(
                browser.read(cx).terminal_directory(),
                Some(directory.as_path())
            );
            if menu {
                click(window, cx, point(px(300.), px(300.)), MouseButton::Right);
            } else {
                window.click("info", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        if menu {
            cx.update_window(handle.into(), |_, window, cx| {
                window.render_frame(cx);
                window.within("popup-menu").click(7usize, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_some());
            window.click("copy-properties", cx);
        })
        .unwrap();
        let text = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(text.contains("Type: Folder"));
        assert!(text.contains(&format!("Path: {}", directory.display())));
        assert!(text.contains(&format!(
            "Modified: {}",
            format_modified_seconds(fs::metadata(&directory).unwrap().modified().ok())
        )));
        cx.update_window(handle.into(), |_, window, cx| window.press("escape", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            if menu {
                click(window, cx, point(px(300.), px(300.)), MouseButton::Right);
            } else {
                window.click("terminal", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        if menu {
            cx.update_window(handle.into(), |_, window, cx| {
                window.render_frame(cx);
                window.within("popup-menu").click(11usize, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        let marker = directory.join("terminal-marker");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists());
        fs::remove_file(marker).unwrap();
    }
    cx.update(|cx| {
        browser.update(cx, |browser, _| {
            browser.terminal = TerminalConfig::Disabled;
            assert!(!browser.can_open_terminal());
        });
    });
}

#[gpui_kit::test]
fn info_dialog_copies_exact_size_and_modified_seconds(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let path = PathBuf::from("/tmp/rs7z-ui-details.txt");
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.files = vec![FileItem {
                path: path.clone(),
                name: "details.txt".into(),
                kind: FileKind::Other,
                size: 10240,
                modified: Some(UNIX_EPOCH),
                created: None,
                accessed: None,
                archive: None,
            }];
            view.selection.context_click(path.clone());
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| browser.show_info(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("dialog").is_some());
        window.click("copy-properties", cx);
    })
    .unwrap();
    let text = cx.read_from_clipboard().unwrap().text().unwrap();
    assert!(text.contains("Size: 10240 (10 KB)"));
    assert!(text.contains(&format!(
        "Modified: {}",
        format_modified_seconds(Some(UNIX_EPOCH))
    )));
}

#[gpui_kit::test]
fn error_dialog_supports_selection_copy_all_and_close(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    let message = "Cannot extract <资料.zip> & file.txt\r\n\r\n    Permission denied\n";
    for close in ["close", "ok", "escape"] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.show_error("Extraction failed", message.into(), window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.within("dialog").find("close").visible());
            let target = error_details_target(window);
            let details = window.find(target.clone());
            assert_eq!(details.value(), Some(message));
            let bounds = details.bounds();
            assert!(bounds.size.height > px(10.), "text bounds: {bounds:?}");
            window.drag(
                point(bounds.left() + px(12.), bounds.top() + px(18.)),
                point(bounds.right() - px(16.), bounds.top() + px(18.)),
                cx,
            );
            window.press("secondary-c", cx);
            let selected = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .expect("the error text should support selection and copying");
            assert!(
                selected.contains("Cannot extract <资料.zip> & file.txt"),
                "{selected:?}"
            );
            assert!(
                !selected.contains("Permission denied"),
                "copy should use only the selection: {selected:?}"
            );
            window.press("secondary-a", cx);
            window.press("secondary-c", cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(message)
            );
            window.input("replacement", cx);
            window.press("backspace", cx);
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("replacement".into()));
            window.press("secondary-v", cx);
            assert_eq!(window.find(target).value(), Some(message));
            window.click("copy-error", cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(message)
            );
            assert!(window.try_find("dialog").is_some());
            if close == "escape" {
                window.press("escape", cx);
            } else {
                window.within("dialog").click(close, cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_none());
            assert!(browser.read(cx).error.is_none());
        })
        .unwrap();
    }
}

fn error_details_target(window: &Window) -> gpui_kit::ElementId {
    // Textarea uses its state entity as its GPUI ID; resolve it by the native label.
    let snapshots = gpui_kit::base::test_support::snapshots(window);
    let mut details = snapshots
        .iter()
        .filter(|snapshot| snapshot.label() == Some("Error details"));
    let target = details.next().expect("error details textarea");
    assert!(details.next().is_none(), "error details should be unique");
    target.path().last().unwrap().clone()
}

fn copy_first_visible_error_line(window: &mut Window, cx: &mut App) -> String {
    let bounds = window.find(error_details_target(window)).bounds();
    window.drag(
        point(bounds.left() + px(12.), bounds.top() + px(18.)),
        point(bounds.right() - px(16.), bounds.top() + px(18.)),
        cx,
    );
    window.press("secondary-c", cx);
    cx.read_from_clipboard().unwrap().text().unwrap()
}

#[gpui_kit::test]
fn long_error_dialog_scrolls_without_hiding_controls(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let message = (0..100)
        .map(|line| {
            if line == 50 {
                format!("line {line:03}: {}", "long error detail ".repeat(100))
            } else {
                format!("line {line:03}: cannot extract <资料.zip> & file.txt")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    cx.update_window(handle.into(), |_, window, cx| {
        browser.unwrap().update(cx, |browser, cx| {
            browser.show_error("Extraction failed", message.clone(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    let bounds = cx
        .update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let target = error_details_target(window);
            let bounds = window.find(target.clone()).bounds();
            assert!(bounds.size.height <= window.rem_size() * 20.);
            assert!(bounds.size.height > window.rem_size() * 2.);
            let copy = window.find("copy-error");
            let ok = window.within("dialog").find("ok");
            assert!(copy.visible() && ok.visible());
            assert!(copy.bounds().top() >= bounds.bottom());
            assert!(copy.bounds().bottom() < px(660.));
            assert!(ok.bounds().bottom() < px(660.));

            assert_eq!(
                copy_first_visible_error_line(window, cx),
                message.lines().next().unwrap()
            );
            let details = window
                .focused_input(cx)
                .unwrap()
                .as_textarea()
                .unwrap()
                .clone();

            // Reveal the native scrollbar in the platform's scrolling-only mode,
            // then drag its thumb from the top to the bottom.
            window.scroll(target.clone(), ScrollDelta::Lines(point(0., -1.)), cx);
            window.drag(
                point(bounds.right() - px(10.), bounds.top() + px(12.)),
                point(bounds.right() - px(10.), bounds.bottom() - px(12.)),
                cx,
            );
            assert!(details.read(cx).scroll_offset().y < -bounds.size.height);
            bounds
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let target = error_details_target(window);
        let bottom_line = copy_first_visible_error_line(window, cx);
        let line_number: usize = bottom_line
            .strip_prefix("line ")
            .and_then(|line| line.split(':').next())
            .and_then(|line| line.parse().ok())
            .unwrap_or_else(|| panic!("expected a visible error line: {bottom_line:?}"));
        assert!(
            line_number > 80,
            "scrollbar did not reach the end: {bottom_line}"
        );

        // Wheel scrolling must return to the original first line.
        window.scroll(target.clone(), ScrollDelta::Lines(point(0., 1000.)), cx);
        assert_eq!(
            copy_first_visible_error_line(window, cx),
            message.lines().next().unwrap()
        );
        assert_eq!(window.find(target.clone()).bounds(), bounds);
        assert_eq!(window.find(target).value(), Some(message.as_str()));
        window.click("copy-error", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(message.as_str())
        );
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn archive_test_shows_success_and_failure_results(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let good = temporary.path().join("good.zip");
    let mut zip = ZipWriter::new(File::create(&good).unwrap());
    zip.add_directory("docs/", SimpleFileOptions::default())
        .unwrap();
    zip.start_file("docs/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&vec![b'x'; 2048]).unwrap();
    zip.finish().unwrap();
    let bad = temporary.path().join("bad.zip");
    fs::write(&bad, b"not a zip archive").unwrap();

    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for (path, folders, files, size, status) in [
        (
            &good,
            "Folder: 1",
            "File: 1",
            "Size: 2048 B (2.0 KB)",
            "Test OK!",
        ),
        (&bad, "Folder: --", "File: --", "Size: --", "Test Failed:"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.run_operation(
                    super::Operation::Test,
                    vec![path.clone()],
                    None,
                    Default::default(),
                    window,
                    cx,
                );
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(window.try_find("dialog").is_some());
            assert_eq!(
                window.find("test-result-filename").label(),
                Some(format!("Filename: {}", path.file_name().unwrap().to_string_lossy()).as_str())
            );
            assert_eq!(window.find("test-result-folders").label(), Some(folders));
            assert_eq!(window.find("test-result-files").label(), Some(files));
            assert_eq!(window.find("test-result-size").label(), Some(size));
            let elapsed = window.find("test-result-elapsed-value");
            assert!(elapsed.label().unwrap().ends_with('s'));
            assert!(elapsed.bounds().top() > window.find("test-result-size-value").bounds().top());
            assert_eq!(window.find("test-result-status").label(), Some(status));
            let value_x = window.find("test-result-filename-value").bounds().left();
            for id in [
                "test-result-folders-value",
                "test-result-files-value",
                "test-result-size-value",
                "test-result-elapsed-value",
            ] {
                assert_eq!(window.find(id).bounds().left(), value_x);
            }
            let filename = path.file_name().unwrap().to_string_lossy();
            window.click(format!("test-result-size-copy-{filename}"), cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(size.strip_prefix("Size: ").unwrap())
            );
            let copy_button = window.find(format!("copy-test-result-{filename}"));
            assert!(
                copy_button.bounds().top() >= window.find("test-result-status").bounds().bottom()
            );
            window.click(format!("copy-test-result-{filename}"), cx);
            let copied = cx.read_from_clipboard().unwrap().text().unwrap();
            assert!(copied.starts_with(&format!(
                "Filename: {}\n",
                path.file_name().unwrap().to_string_lossy()
            )));
            assert!(copied.contains(&format!("\n{size}\n")));
            assert!(copied.contains("\nElapsed: "));
            assert!(copied.contains(status));
            assert!(window.within("dialog").find("close").visible());
            if path == &bad {
                let details = gpui_kit::base::test_support::snapshots(window);
                assert!(details.iter().any(|item| {
                    item.label() == Some("Test error details")
                        && item.value().is_some_and(|value| !value.is_empty())
                        && copy_button.bounds().top() >= item.bounds().bottom()
                }));
            }
            if path == &good {
                window.within("dialog").click("close", cx);
            } else {
                window.click("cancel-progress", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn archive_test_progress_shows_elapsed_and_unknown_remaining(cx: &mut TestAppContext) {
    use super::progress_dialog::{OperationProgressDialog, TestResult};
    use rs7z::service::operations::ProgressHandle;

    cx.update(init_test_app);
    let mut state = None;
    let handle = cx.open_window(size(px(820.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            OperationProgressDialog::new(
                ProgressHandle::default(),
                Some(Instant::now() - Duration::from_secs(100)),
                window,
                cx,
            )
        });
        state = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("operation-progress-elapsed").label(),
            Some("1m40s")
        );
        assert_eq!(
            window.find("operation-progress-remaining").label(),
            Some("--")
        );
        state.as_ref().unwrap().update(cx, |state, cx| {
            state.finish(
                &Ok(()),
                Some(TestResult::new(
                    std::path::Path::new("example.zip"),
                    None,
                    None,
                    Duration::from_millis(100_123),
                )),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("operation-progress-elapsed").is_none());
        assert_eq!(
            window.find("test-result-elapsed-value").label(),
            Some("100.123s")
        );
        window.click("test-result-elapsed-copy-example.zip", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("100.123s")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_prompts_for_encrypted_archives_before_running(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    let source = directory.join("item.txt");
    fs::write(&source, b"encrypted contents").unwrap();
    for (name, format, encryption) in [
        ("encrypted.zip", "-tzip", "-mem=ZipCrypto"),
        ("encrypted.7z", "-t7z", "-mhe=on"),
    ] {
        let output = rs7z::service::seven_zip_cli::command()
            .unwrap()
            .args(["a", format, "-psecret", encryption, "--"])
            .arg(directory.join(name))
            .arg(&source)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();

    for name in ["encrypted.zip", "encrypted.7z"] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, name, window, cx);
            window.click("test", cx);
            assert!(window.try_find("confirm-operation").is_none());
            assert!(window.try_find("choose-source-file").is_none());
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("archive-open-password").is_some());
            assert!(window.try_find("test-result-status").is_none());
            edit_text("archive-open-password", "secret", window, cx);
            window.click("unlock-archive", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("test-result-status").label(), Some("Test OK!"));
            window.click("cancel-progress", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn operation_progress_dialog_cancels_before_closing(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for title in ["Adding to archive", "Extracting archive"] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.busy = true;
                browser.launch_progress(
                    title,
                    false,
                    |progress| {
                        let started = Instant::now();
                        while !progress.is_cancelled() && started.elapsed() < Duration::from_secs(3)
                        {
                            thread::sleep(Duration::from_millis(10));
                        }
                        if progress.is_cancelled() {
                            Err(OperationError::Cancelled)
                        } else {
                            Ok(())
                        }
                    },
                    window,
                    cx,
                );
            });
        })
        .unwrap();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_some());
            assert!(window.try_find("cancel-progress").is_some());
            let progress = window.find("progress-width").bounds().size;
            assert!(progress.width > window.rem_size() * 27.);
            assert_eq!(progress.height, px(14.));
            assert!(window.find("operation-progress-elapsed").visible());
            assert_eq!(
                window.find("operation-progress-remaining").label(),
                Some("--")
            );
            assert!(browser.read(cx).busy);
            window.click("cancel-progress", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(window.try_find("dialog").is_none());
            assert!(
                browser
                    .read(cx)
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cancelled")
            );
        })
        .unwrap();
    }

    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.queue_progress("Testing queue", false, |_| Ok(()), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert_eq!(
            browser.read(cx).status.as_deref(),
            Some("Testing queue complete")
        );
        assert!(window.try_find("dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn missing_compression_tool_error_keeps_install_commands_copyable(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    // Exercise the operation failure route with a dependency-error fixture.
    let message = "Required command 'xz' is not installed or could not be found.\n\nInstall it, then try again:\n\nbrew install xz zstd\n\nMake sure 'xz' is available in PATH. Restart rs7z if PATH changes.";
    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.queue_progress(
                "Adding to archive",
                false,
                move |_| Err(OperationError::Failed(message.into())),
                window,
                cx,
            );
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(window.try_find("cancel-progress").is_none());
        let target = error_details_target(window);
        assert_eq!(window.find(target.clone()).value(), Some(message));
        window.click(target.clone(), cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(message)
        );
        window.input("do not edit", cx);
        assert_eq!(window.find(target).value(), Some(message));
        window.click("copy-error", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(message)
        );
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn rar_output_dialog_retains_copyable_output_and_errors(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("sample.rar");
    std::fs::write(&archive, include_bytes!("../fixtures/sample-rar5.rar")).unwrap();
    let destination = root.path().join("output");
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    // The second attempt fails preflight because the destination is nonempty.
    for expected in ["Everything is Ok", "Destination folder is not empty"] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.run_operation(
                    super::Operation::Extract,
                    vec![archive.clone()],
                    Some(destination.clone()),
                    Default::default(),
                    window,
                    cx,
                );
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(window.try_find("dialog").is_some());
            assert!(window.try_find("operation-progress").is_none());
            let snapshots = gpui_kit::base::test_support::snapshots(window);
            let target = snapshots
                .iter()
                .find(|item| item.label() == Some("7-Zip output"))
                .expect("CLI output textarea")
                .path()
                .last()
                .unwrap()
                .clone();
            let output = window.find(target.clone()).value().unwrap().to_owned();
            assert!(output.contains(expected), "{output}");
            let bounds = window.find(target.clone()).bounds();
            assert!(bounds.size.height <= window.rem_size() * 17.);
            assert!(window.find("cancel-progress").bounds().bottom() < px(660.));
            window.click(target.clone(), cx);
            window.press("secondary-a", cx);
            window.press("secondary-c", cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(output.as_str())
            );
            window.input("must remain readonly", cx);
            assert_eq!(window.find(target).value(), Some(output.as_str()));
            window.click("cancel-progress", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_none());
        })
        .unwrap();
    }
    assert_eq!(
        std::fs::read(destination.join("helloworld.txt")).unwrap(),
        b"hello libarchive test suite!\n"
    );
}

#[gpui_kit::test]
fn output_dialog_cancels_then_waits_for_explicit_close(cx: &mut TestAppContext) {
    cx.update(init_test_app);
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| Browser::new(window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.busy = true;
            browser.launch_progress(
                "Extracting archive",
                true,
                |progress| {
                    let started = Instant::now();
                    while !progress.is_cancelled() && started.elapsed() < Duration::from_secs(3) {
                        thread::sleep(Duration::from_millis(10));
                    }
                    assert!(progress.is_cancelled());
                    Err(OperationError::Cancelled)
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(browser.read(cx).busy);
        assert!(window.try_find("operation-progress").is_none());
        assert!(window.find("operation-progress-elapsed").visible());
        assert_eq!(
            window.find("operation-progress-remaining").label(),
            Some("--")
        );

        window.click("cancel-progress", cx);
        assert!(window.try_find("dialog").is_some());
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(
            browser
                .read(cx)
                .status
                .as_deref()
                .unwrap()
                .contains("cancelled")
        );
        assert!(window.try_find("dialog").is_some());

        let elapsed = window
            .find("operation-progress-elapsed")
            .label()
            .unwrap()
            .to_owned();
        window.render_frame(cx);
        assert_eq!(
            window.find("operation-progress-elapsed").label(),
            Some(elapsed.as_str())
        );
        assert_eq!(
            window.find("operation-progress-remaining").label(),
            Some("0s")
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn archive_add_prefills_the_selected_directory(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    let temporary = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temporary.path()).unwrap().join("add.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("docs/nested/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    for (directory, selected, expected) in [
        ("", None, "/"),
        ("", Some("docs"), "/docs"),
        ("docs", None, "/docs"),
        ("docs", Some("nested"), "/docs/nested"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.navigate(&path.join(directory), window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            if let Some(selected) = selected {
                select_item(&browser, selected, window, cx);
            }
            window.click("add", cx);
            assert_eq!(
                window.find("archive-directory-path").value(),
                Some(expected)
            );
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn archive_system_transfers_default_to_parent_and_accept_typed_paths(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let path = parent.join("transfer.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("nested/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path.join("nested"), window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    for action in ["copy", "move"] {
        let destination = parent.join(format!("{action} target 中文"));
        fs::create_dir(&destination).unwrap();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            select_item(&browser, "item.txt", window, cx);
            window.click(action, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("archive-directory-path").is_none());
            let input = window.find("archive-system-path");
            assert_eq!(input.value(), Some(parent.to_str().unwrap()));
            assert_eq!(input.focused(), Some(true));
            // Typing immediately proves the caret starts at the end of the default path.
            window.input(" suffix", cx);
            assert_eq!(
                window.find("archive-system-path").value(),
                Some(format!("{} suffix", parent.display()).as_str())
            );
            edit_text("archive-system-path", "missing directory", window, cx);
            window.click("confirm-archive-operation", cx);
            assert!(window.try_find("confirm-archive-operation").is_some());
            assert!(!browser.read(cx).busy);
            // Relative paths resolve against the archive's real parent, including Unicode/spaces.
            edit_text(
                "archive-system-path",
                destination.file_name().unwrap().to_str().unwrap(),
                window,
                cx,
            );
            window.click("confirm-archive-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            assert!(window.try_find("dialog").is_none());
        })
        .unwrap();
        assert_eq!(fs::read(destination.join("item.txt")).unwrap(), b"contents");
        let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(zip.by_name("nested/item.txt").is_ok(), action == "copy");
    }
}

#[gpui_kit::test]
fn encrypted_archive_navigation_requires_verified_password(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let path = parent.join("protected.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "nested/item.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
    )
    .unwrap();
    zip.write_all(b"private contents").unwrap();
    zip.finish().unwrap();
    let original = fs::read(&path).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for entry in ["double-click", "typed-path"] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            if entry == "double-click" {
                window.double_click(format!("file-{}", path.display()), cx);
            } else {
                let target = ("input", browser.read(cx).path_input.entity_id());
                window.click(target, cx);
                window.press("secondary-a", cx);
                window.input(&path.join("nested").display().to_string(), cx);
                window.press("enter", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, parent);
            assert!(browser.read(cx).archive.is_none());
            assert_eq!(window.find("archive-open-password").focused(), Some(true));
            window.input("incorrect", cx);
            window.click("unlock-archive", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, parent);
            assert!(browser.read(cx).archive.is_none());
            assert!(
                window
                    .find("archive-password-error")
                    .label()
                    .unwrap()
                    .contains("password")
            );
            assert_eq!(window.find("archive-open-password").focused(), Some(true));
            // The rejected password is selected so typing replaces it.
            window.input("secret", cx);
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_none());
            let expected = if entry == "double-click" {
                path.clone()
            } else {
                path.join("nested")
            };
            assert_eq!(browser.read(cx).current_dir, expected);
            assert_eq!(browser.read(cx).archive_password.as_deref(), Some("secret"));
            if entry == "double-click" {
                window.double_click(format!("file-{}", path.join("nested").display()), cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, path.join("nested"));
            assert!(window.try_find("dialog").is_none());
            select_item(&browser, "item.txt", window, cx);
            window.click("copy", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Authenticated browsing supplies the password to subsequent transfers.
            window.click("confirm-archive-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            browser.update(cx, |browser, cx| browser.navigate(&parent, window, cx));
            assert!(browser.read(cx).archive_password.is_none());
        })
        .unwrap();
        assert_eq!(
            fs::read(parent.join("item.txt")).unwrap(),
            b"private contents"
        );
        fs::remove_file(parent.join("item.txt")).unwrap();
    }
    assert_eq!(fs::read(&path).unwrap(), original);
    // Cancellation leaves the original location/history intact and does not unlock.
    let history = cx.update(|cx| browser.read(cx).history.clone());
    cx.update_window(handle.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| browser.navigate(&path, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("dialog").is_none());
        assert_eq!(browser.read(cx).current_dir, parent);
        assert_eq!(browser.read(cx).history, history);
        assert!(browser.read(cx).archive_password.is_none());
        assert_eq!(
            browser.read(cx).path_input.read(cx).value().as_str(),
            parent.to_str().unwrap()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tar_format_selector_creates_posix_tar(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"tar contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        window.click("add", cx);
        window.within("archive-format").click("input", cx);
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("operation-name").value(), Some("item.tar"));
        assert!(window.try_find("tar-format").is_some());
        assert!(window.try_find("zip-compression").is_none());
        window.within("tar-format").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let bytes = fs::read(directory.join("item.tar")).unwrap();
    assert_eq!(&bytes[257..263], b"ustar\0");
}

#[gpui_kit::test]
fn seven_zip_method_selector_updates_related_fields(cx: &mut TestAppContext) {
    use std::fs;

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    fs::write(directory.join("item.txt"), b"7z contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "item.txt", window, cx);
        window.click("add", cx);
        window.within("archive-format").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.scroll("dialog", ScrollDelta::Lines(point(0., -20.)), cx);
        for id in ["7z-method", "7z-dictionary", "7z-word", "7z-solid"] {
            assert!(window.try_find(id).is_some(), "missing {id}");
        }
        let level = window.find("zip-compression").bounds();
        let volume = window.find("archive-volume-size-group").bounds();
        let method = window.find("7z-method").bounds();
        assert!(level.bottom() < volume.top());
        assert!(volume.bottom() < method.top());
        assert_eq!(level.left(), volume.left());
        window.within("7z-method").click("input", cx);
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("7z-word").is_some());
        window.within("7z-method").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("7z-dictionary").is_none());
        assert!(window.try_find("7z-word").is_none());
        assert!(window.try_find("7z-solid").is_some());
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let path = directory.join("item.7z");
    assert!(path.exists());
    let listing = rs7z::service::seven_zip_cli::command()
        .unwrap()
        .args(["l", "-slt", "--"])
        .arg(path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&listing.stdout).contains("Method = BZip2"));
}

#[gpui_kit::test]
fn seven_zip_creation_dialog_switches_format_and_creates_volumes(cx: &mut TestAppContext) {
    creation_dialog_creates_volumes(cx, "7z");
}

#[gpui_kit::test]
fn zip_creation_dialog_creates_and_browses_readonly_volumes(cx: &mut TestAppContext) {
    creation_dialog_creates_volumes(cx, "zip");
}

fn creation_dialog_creates_volumes(cx: &mut TestAppContext, format: &str) {
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    fs::write(parent.join("large.bin"), vec![42u8; 8192]).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        select_item(&browser, "large.bin", window, cx);
        window.click("add", cx);
        assert_eq!(window.find("operation-name").value(), Some("large.zip"));
        assert!(window.try_find("archive-volume-size").is_some());
        if format == "7z" {
            window.within("archive-format").click("input", cx);
            window.press("down", cx);
            window.press("enter", cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("operation-name").value(),
            Some(format!("large.{format}").as_str())
        );
        assert!(window.try_find("archive-volume-size").is_some());
        window.scroll("dialog", ScrollDelta::Lines(point(0., -20.)), cx);
        window.click("archive-volume-size-presets", cx);
        let mut menu = window.within("popup-menu");
        for (index, label) in [
            "10M",
            "100M",
            "1000M",
            "650M - CD",
            "700M - CD",
            "4092M - FAT",
            "4480M - DVD",
            "8128M - DVD DL",
            "23040M - BD",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(menu.find(index).label(), Some(label));
        }
        menu.click(3usize, cx);
        assert_eq!(window.find("archive-volume-size").value(), Some("650M"));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("archive-volume-size-presets", cx);
        window.within("popup-menu").click(8usize, cx);
        assert_eq!(window.find("archive-volume-size").value(), Some("23040M"));
        edit_text("archive-volume-size", "0M", window, cx);
        window.click("confirm-operation", cx);
        assert!(window.try_find("confirm-operation").is_some());
        assert!(!browser.read(cx).busy);
        edit_text("archive-volume-size", "1K", window, cx);
        window.scroll("dialog", ScrollDelta::Lines(point(0., 20.)), cx);
        window.within("zip-compression").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        if format == "7z" {
            for id in ["7z-method", "7z-dictionary", "7z-word", "7z-solid"] {
                assert!(
                    window.try_find(id).is_none(),
                    "unexpected {id} in Store mode"
                );
            }
        }
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(parent.join(format!("large.{format}.001")).exists());
    assert!(parent.join(format!("large.{format}.002")).exists());
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            browser.read(cx).error.is_none(),
            "{:?}",
            browser.read(cx).error
        );
        window.double_click(
            format!(
                "file-{}",
                parent.join(format!("large.{format}.001")).display()
            ),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).can_add());
        select_item(&browser, "large.bin", window, cx);
        for id in ["add", "move", "delete", "rename"] {
            window.click(id, cx);
            assert!(
                window.try_find("dialog").is_none(),
                "{id} should be unavailable"
            );
        }
        window.click("copy", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("archive-system-path").value(),
            Some(parent.to_str().unwrap())
        );
        assert_eq!(window.find("archive-system-path").focused(), Some(true));
        window.within("archive-destination-kind").click(0usize, cx);
        assert!(window.try_find("archive-directory-path").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn tar_creation_formats_hide_password_and_volume_controls_and_browse_nested_paths(
    cx: &mut TestAppContext,
) {
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    fs::create_dir_all(parent.join("docs/nested")).unwrap();
    fs::write(parent.join("docs/nested/item.txt"), b"contents").unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for (steps, suffix) in [
        (3, "tar.gz"),
        (4, "tar.bz2"),
        (5, "tar.xz"),
        (6, "tar.lzma"),
        (7, "tar.zst"),
    ] {
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(&browser, "docs", window, cx);
            window.click("add", cx);
            assert!(window.try_find("zip-password").is_some());
            window.within("archive-format").click("input", cx);
            for _ in 0..steps {
                window.press("down", cx);
            }
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let name = format!("docs.{suffix}");
            assert_eq!(window.find("operation-name").value(), Some(name.as_str()));
            assert!(window.try_find("zip-password").is_none());
            assert!(window.try_find("archive-volume-size").is_none());
            assert!(window.try_find("zip-compression").is_some());
            window.click("confirm-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let archive = parent.join(format!("docs.{suffix}"));
        assert!(archive.exists());
        cx.update_window(handle.into(), |_, window, cx| {
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            select_item(&browser, &format!("docs.{suffix}"), window, cx);
            window.press("alt-e", cx);
            assert_eq!(
                window.find("extract-destination").value(),
                Some(parent.join("docs").to_str().unwrap())
            );
            assert!(window.try_find("zip-password").is_none());
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.double_click(format!("file-{}", archive.display()), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(browser.read(cx).current_dir, archive);
            assert!(browser.read(cx).archive_mutable());
            let path = archive.join("docs/nested");
            let input = browser.read(cx).path_input.clone();
            window.click(("input", input.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input(path.to_str().unwrap(), cx);
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(browser.read(cx).current_dir, archive.join("docs/nested"));
            assert_eq!(browser.read(cx).files[0].name, "item.txt");
            browser.update(cx, |browser, cx| browser.navigate(&parent, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn tar_compress_browsing_disables_mutations_in_toolbar_menu_and_shortcuts(cx: &mut TestAppContext) {
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let archive = parent.join("legacy.taz");
    fs::write(&archive, include_bytes!("../fixtures/sample.tar.Z")).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.double_click(format!("file-{}", archive.display()), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).current_dir, archive);
        select_item(&browser, "docs", window, cx);
        for id in ["add", "move", "delete", "rename"] {
            window.click(id, cx);
            assert!(window.try_find("dialog").is_none(), "{id}");
        }
        for key in ["alt-a", "alt-m", "alt-d", "alt-r", "delete"] {
            window.press(key, cx);
            assert!(window.try_find("dialog").is_none(), "{key}");
        }
        click(window, cx, point(px(300.), px(160.)), MouseButton::Right);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        for ix in [0usize, 2, 3, 4] {
            window.within("popup-menu").click(ix, cx);
            assert!(window.try_find("dialog").is_none());
        }
        window.within("popup-menu").click(1usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("archive-system-path").value(),
            Some(parent.to_str().unwrap())
        );
        assert_eq!(window.find("archive-system-path").focused(), Some(true));
        assert!(window.try_find("zip-password").is_none());
        window.within("archive-destination-kind").click(0usize, cx);
        assert!(window.try_find("archive-directory-path").is_none());
        window.click("confirm-archive-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        fs::read(parent.join("docs/item.txt")).unwrap(),
        b"legacy tar contents\n"
    );
}

#[gpui_kit::test]
fn zip_alias_and_split_zip_navigation_preserve_capabilities(cx: &mut TestAppContext) {
    use std::{fs, io::Write, process::Command};
    use zip::{ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let alias = parent.join("document.docx");
    let mut zip = ZipWriter::new(fs::File::create(&alias).unwrap());
    zip.start_file("docs/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"alias contents").unwrap();
    zip.finish().unwrap();
    fs::create_dir(parent.join("docs")).unwrap();
    fs::write(parent.join("docs/item.txt"), vec![42; 180_000]).unwrap();
    let command = Command::new("zip")
        .current_dir(&parent)
        .args(["-0", "-s", "64k", "bundle.zip", "docs/item.txt"])
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for split in [false, true] {
        let source = if split {
            parent.join("bundle.z01")
        } else {
            alias.clone()
        };
        cx.update_window(handle.into(), |_, window, cx| {
            select_item(
                &browser,
                source.file_name().unwrap().to_str().unwrap(),
                window,
                cx,
            );
            assert!(browser.read(cx).can_extract_or_test());
            assert!(browser.read(cx).is_testable());
            if split {
                let input = browser.read(cx).path_input.clone();
                window.click(("input", input.entity_id()), cx);
                window.press("secondary-a", cx);
                window.input(source.join("docs").to_str().unwrap(), cx);
                window.press("enter", cx);
            } else {
                window.double_click(format!("file-{}", source.display()), cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        if !split {
            cx.update_window(handle.into(), |_, window, cx| {
                window.double_click(format!("file-{}", source.join("docs").display()), cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let archive_path = if split {
                parent.join("bundle.zip")
            } else {
                source.clone()
            };
            assert_eq!(browser.read(cx).current_dir, archive_path.join("docs"));
            assert_eq!(browser.read(cx).archive.as_ref().unwrap().read_only, split);
            assert_eq!(browser.read(cx).archive_mutable(), !split);
            select_item(&browser, "item.txt", window, cx);
            if split {
                for id in ["add", "move", "delete", "rename"] {
                    window.click(id, cx);
                    assert!(window.try_find("dialog").is_none(), "{id}");
                }
            }
            window.click("copy", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("archive-system-path").focused(), Some(true));
            if split {
                window.within("archive-destination-kind").click(0usize, cx);
                window.render_frame(cx);
                assert!(window.try_find("archive-system-path").is_some());
                assert!(window.try_find("archive-directory-path").is_none());
            }
            let export = parent.join(if split {
                "split-export"
            } else {
                "alias-export"
            });
            fs::create_dir(&export).unwrap();
            edit_text("archive-system-path", export.to_str().unwrap(), window, cx);
            window.click("confirm-archive-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let exported = fs::read(parent.join(if split {
            "split-export/item.txt"
        } else {
            "alias-export/item.txt"
        }))
        .unwrap();
        assert_eq!(
            exported,
            if split {
                vec![42; 180_000]
            } else {
                b"alias contents".to_vec()
            }
        );
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| browser.navigate(&parent, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn startup_path_opens_folders_and_archives_with_normal_password_prompts(cx: &mut TestAppContext) {
    use rs7z::{
        model::archive::ArchiveOptions,
        service::operations::{self, ProgressHandle},
    };
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let folder = parent.join("startup folder");
    fs::create_dir(&folder).unwrap();
    let source = folder.join("hello.txt");
    fs::write(&source, b"startup contents").unwrap();
    let zip = parent.join("startup.zip");
    let tar = parent.join("startup.tgz");
    let encrypted = parent.join("encrypted.zip");
    for archive in [&zip, &tar] {
        operations::add_many_with_progress(
            std::slice::from_ref(&source),
            archive,
            &ProgressHandle::default(),
        )
        .unwrap();
    }
    let mut options = ArchiveOptions::default();
    options.password = Some("startup password".into());
    operations::add_many_with_options(&[source], &encrypted, &options, &ProgressHandle::default())
        .unwrap();
    for path in [&folder, &zip, &tar, &encrypted] {
        let mut browser = None;
        let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
            let view = cx.new(|cx| Browser::new_at(path, window, cx));
            browser = Some(view.clone());
            Root::new(view, window, cx)
        });
        let browser = browser.unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).busy);
            assert!(browser.read(cx).error.is_none());
            if path == &encrypted {
                assert!(window.try_find("dialog").is_some());
                assert!(browser.read(cx).archive.is_none());
                edit_text("archive-open-password", "startup password", window, cx);
                window.click("unlock-archive", cx);
            } else {
                assert_eq!(&browser.read(cx).current_dir, path);
                assert_eq!(browser.read(cx).archive.is_some(), path != &folder);
                assert!(
                    browser
                        .read(cx)
                        .files
                        .iter()
                        .any(|item| item.name == "hello.txt")
                );
            }
        })
        .unwrap();
        if path == &encrypted {
            cx.run_until_parked();
            cx.update_window(handle.into(), |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(&browser.read(cx).current_dir, path);
                assert!(browser.read(cx).archive.is_some());
                assert!(
                    browser
                        .read(cx)
                        .files
                        .iter()
                        .any(|item| item.name == "hello.txt")
                );
                assert!(window.try_find("dialog").is_none());
            })
            .unwrap();
        }
    }
}

#[gpui_kit::test]
fn standalone_streams_open_extract_dialog_and_write_to_parent(cx: &mut TestAppContext) {
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let input = parent.join("fixture.txt");
    fs::write(&input, b"hello").unwrap();
    let source = parent.join("report.txt.gz");
    let result = rs7z::service::seven_zip_cli::command()
        .unwrap()
        .args(["a", "-tgzip", "--"])
        .arg(&source)
        .arg(&input)
        .output()
        .unwrap();
    assert!(result.status.success());
    fs::remove_file(input).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "report.txt.gz", window, cx);
        assert!(browser.read(cx).can_extract_or_test());
        assert!(!browser.read(cx).is_testable());
        window.click("test", cx);
        window.press("alt-t", cx);
        assert!(window.try_find("dialog").is_none());
        window.double_click(format!("file-{}", source.display()), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, parent);
        assert!(browser.read(cx).archive.is_none());
        assert_eq!(
            window.find("extract-destination").value(),
            Some(parent.to_str().unwrap())
        );
        assert_eq!(window.find("extract-destination").focused(), Some(true));
        for id in [
            "zip-password",
            "zip-compression",
            "archive-volume-size",
            "archive-format",
        ] {
            assert!(
                window.try_find(id).is_none(),
                "unexpected stream control: {id}"
            );
        }
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(fs::read(parent.join("report.txt")).unwrap(), b"hello");
    assert!(source.exists());
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(browser.read(cx).error.is_none());
        select_item(&browser, "report.txt.gz", window, cx);
        window.click("extract", cx);
        window.click("confirm-operation", cx);
        assert!(window.try_find("dialog").is_some());
        assert!(!browser.read(cx).busy);
    })
    .unwrap();
    assert_eq!(fs::read(parent.join("report.txt")).unwrap(), b"hello");
}

#[gpui_kit::test]
fn rar_extraction_works_from_toolbar_menu_double_click_and_typed_path(cx: &mut TestAppContext) {
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let archive = parent.join("sample.rar");
    fs::write(&archive, include_bytes!("../fixtures/sample-rar5.rar")).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "sample.rar", window, cx);
        window.click("test", cx);
        window.press("alt-t", cx);
        assert!(window.try_find("dialog").is_none());
        window.click("extract", cx);
        assert!(window.try_find("extract-destination").is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    for typed in [false, true] {
        cx.update_window(handle.into(), |_, window, cx| {
            if typed {
                let input = browser.read(cx).path_input.clone();
                window.click(("input", input.entity_id()), cx);
                window.press("secondary-a", cx);
                window.input(archive.to_str().unwrap(), cx);
                window.press("enter", cx);
            } else {
                window.double_click(format!("file-{}", archive.display()), cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, parent);
            assert!(browser.read(cx).archive.is_none());
            assert_eq!(
                window.find("extract-destination").value(),
                Some(parent.join("sample").to_str().unwrap())
            );
            assert_eq!(window.find("extract-destination").focused(), Some(true));
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    cx.update_window(handle.into(), |_, window, cx| {
        select_item(&browser, "sample.rar", window, cx);
        click(window, cx, point(px(300.), px(160.)), MouseButton::Right);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.within("popup-menu").click(2usize, cx);
        assert!(window.try_find("dialog").is_none());
        window.within("popup-menu").click(1usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("confirm-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        fs::read(parent.join("sample/helloworld.txt")).unwrap(),
        b"hello libarchive test suite!\n"
    );
}

#[gpui_kit::test]
fn encrypted_seven_zip_headers_are_unlocked_before_nested_navigation(cx: &mut TestAppContext) {
    use rs7z::{
        model::archive::ArchiveOptions,
        service::operations::{ProgressHandle, add_many_with_options},
    };
    use std::fs;
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(temporary.path()).unwrap();
    let source = parent.join("nested");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("item.txt"), b"private contents").unwrap();
    let path = parent.join("protected.7z");
    let mut options = ArchiveOptions::default();
    options.password = Some("secret".into());
    add_many_with_options(&[source], &path, &options, &ProgressHandle::default()).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&parent, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    for entry in ["double-click", "typed-path"] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            if entry == "double-click" {
                window.double_click(format!("file-{}", path.display()), cx);
            } else {
                let target = ("input", browser.read(cx).path_input.entity_id());
                window.click(target, cx);
                window.press("secondary-a", cx);
                window.input(&path.join("nested").display().to_string(), cx);
                window.press("enter", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, parent);
            assert_eq!(window.find("archive-open-password").focused(), Some(true));
            window.input("wrong", cx);
            window.click("unlock-archive", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("archive-password-error").is_some());
            window.input("secret", cx);
            window.click("unlock-archive", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_none());
            assert_eq!(browser.read(cx).archive_password.as_deref(), Some("secret"));
            if entry == "double-click" {
                window.double_click(format!("file-{}", path.join("nested").display()), cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(browser.read(cx).current_dir, path.join("nested"));
            assert!(
                browser.read(cx).error.is_none(),
                "{:?}",
                browser.read(cx).error
            );
            select_item(&browser, "item.txt", window, cx);
            window.click("copy", cx);
            window.click("confirm-archive-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            fs::read(parent.join("item.txt")).unwrap(),
            b"private contents"
        );
        fs::remove_file(parent.join("item.txt")).unwrap();
        cx.update_window(handle.into(), |_, window, cx| {
            browser.update(cx, |browser, cx| browser.navigate(&parent, window, cx));
            assert!(browser.read(cx).archive_password.is_none());
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn archive_navigation_and_copy_through_dialog(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};
    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    let archive = directory.join("browse.zip");
    let mut zip = ZipWriter::new(File::create(&archive).unwrap());
    zip.start_file("docs/item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.add_directory("target/", SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&directory, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, archive);
        assert!(browser.read(cx).archive.is_some());
        for id in ["terminal", "extract", "test"] {
            assert!(window.try_find(id).is_none());
        }
        window.click("copy", cx);
        assert!(window.try_find("confirm-archive-operation").is_none());
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, archive.join("docs"));
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        window.click("copy", cx);
        window.render_frame(cx);
        assert!(window.try_find("confirm-archive-operation").is_some());
        assert!(window.try_find("archive-system-folder").is_some());
        assert!(window.try_find("archive-directory").is_none());
        window.within("archive-destination-kind").click(0usize, cx);
        window.within("archive-directory").click("input", cx);
        window.press("up", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("archive-directory-path").value(), Some("/"));
        window.click("confirm-archive-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(
            browser.read(cx).error.is_none(),
            "{:?}",
            browser.read(cx).error
        );
        window.click("up", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, archive);
        assert!(
            browser
                .read(cx)
                .files
                .iter()
                .any(|file| file.name == "item.txt")
        );
        window.click("back", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, archive.join("docs"));
        assert_eq!(browser.read(cx).files[0].name, "item.txt");
        window.click("forward", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(browser.read(cx).current_dir, archive);
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        window.press("secondary-a", cx);
        click(window, cx, point(px(300.), px(160.)), MouseButton::Right);
        assert_eq!(browser.read(cx).selection.len(), 3);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_menu_shortcuts(
        handle.into(),
        cx,
        &[
            "kbd:alt-a",
            "kbd:alt-c",
            "kbd:alt-m",
            "kbd:alt-d",
            "kbd:alt-r",
            "kbd:alt-i",
        ],
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let mut menu = window.within("popup-menu");
        for (ix, label) in [
            "Add to archive…",
            "Copy…",
            "Move…",
            "Delete",
            "Rename…",
            "Info…",
            "Refresh",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(menu.find(ix).label(), Some(*label));
        }
        assert!(!browser.read(cx).can_add());
        menu.click(0usize, cx);
        menu.click(4usize, cx);
        menu.click(3usize, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).busy);
        assert!(browser.read(cx).error.is_none());
        assert!(browser.read(cx).files.is_empty());
        window.click("add", cx);
        window.click("confirm-archive-operation", cx);
        assert!(window.try_find("confirm-archive-operation").is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("up", cx);
        assert!(browser.read(cx).archive.is_none());
        assert_eq!(browser.read(cx).current_dir, directory);
        assert!(window.try_find("terminal").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn archive_manual_destinations_validate_and_create_directories(cx: &mut TestAppContext) {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

    cx.update(init_test_app);
    cx.update(|cx| cx.set_reduce_motion(true));
    let temporary = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temporary.path())
        .unwrap()
        .join("manual.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("item.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.finish().unwrap();
    let original = fs::read(&path).unwrap();
    let mut browser = None;
    let handle = cx.open_window(size(px(1060.), px(660.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut view = Browser::new(window, cx);
            view.navigate(&path, window, cx);
            view
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        click(window, cx, point(px(300.), px(160.)), MouseButton::Left);
        window.click("copy", cx);
        assert!(window.try_find("archive-system-folder").is_some());
        window.within("archive-destination-kind").click(0usize, cx);
    })
    .unwrap();
    for invalid in ["", "../escape", "/new/.../child", "/bad!", "/new//child"] {
        cx.update_window(handle.into(), |_, window, cx| {
            window.click("archive-directory-path", cx);
            window.press("secondary-a", cx);
            window.press("backspace", cx);
            window.input(invalid, cx);
            window.click("confirm-archive-operation", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("confirm-archive-operation").is_some());
            assert!(!browser.read(cx).busy);
        })
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    cx.update_window(handle.into(), |_, window, cx| {
        window.click("archive-directory-path", cx);
        window.press("secondary-a", cx);
        window.input("资料/new folder", cx);
        window.click("confirm-archive-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    {
        let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert!(zip.by_name("资料/new folder/item.txt").is_ok());
        assert!(zip.by_name("item.txt").is_ok());
    }
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            browser.read(cx).error.is_none(),
            "{:?}",
            browser.read(cx).error
        );
        click(window, cx, point(px(300.), px(192.)), MouseButton::Left);
        window.click("move", cx);
        window.within("archive-destination-kind").click(0usize, cx);
        window.click("archive-directory-path", cx);
        window.press("secondary-a", cx);
        window.input("/moved/deep", cx);
        window.click("confirm-archive-operation", cx);
    })
    .unwrap();
    cx.run_until_parked();
    {
        let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert!(zip.by_name("moved/deep/item.txt").is_ok());
        assert!(zip.by_name("item.txt").is_err());
        assert!(zip.by_name("资料/new folder/item.txt").is_ok());
    }
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            browser.read(cx).error.is_none(),
            "{:?}",
            browser.read(cx).error
        );
        window.click("add", cx);
        assert_eq!(window.find("archive-directory-path").value(), Some("/"));
        assert!(window.try_find("archive-directory").is_some());
        window.press("escape", cx);
    })
    .unwrap();
}
