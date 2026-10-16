#[path = "../src/ui/selection.rs"]
mod selection;

use selection::Selection;
use std::path::PathBuf;

fn paths() -> Vec<PathBuf> {
    ["a", "b", "c", "d"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

#[test]
fn click_toggle_and_range_follow_visible_order() {
    let paths = paths();
    let mut selection = Selection::default();
    selection.click(&paths, paths[0].clone(), false, false);
    selection.click(&paths, paths[2].clone(), true, false);
    assert_eq!(
        selection.ordered(&paths),
        vec![paths[0].clone(), paths[2].clone()]
    );
    selection.click(&paths, paths[3].clone(), false, true);
    assert_eq!(
        selection.ordered(&paths),
        vec![paths[2].clone(), paths[3].clone()]
    );
    selection.click(&paths, paths[2].clone(), true, false);
    assert_eq!(selection.only(), Some(&paths[3]));
}

#[test]
fn keyboard_range_and_select_all() {
    let paths = paths();
    let mut selection = Selection::default();
    selection.step(&paths, true, false);
    selection.step(&paths, true, true);
    selection.step(&paths, true, true);
    assert_eq!(selection.ordered(&paths), paths[..3]);
    assert_eq!(selection.cursor(), Some(&paths[2]));
    selection.select_all(&paths);
    assert_eq!(selection.len(), 4);
    selection.clear();
    assert_eq!(selection.len(), 0);
    assert!(selection.cursor().is_none());
}

#[test]
fn context_click_preserves_selected_group_or_selects_target() {
    let paths = paths();
    let mut selection = Selection::default();
    selection.click(&paths, paths[0].clone(), false, false);
    selection.click(&paths, paths[1].clone(), true, false);
    selection.context_click(paths[0].clone());
    assert_eq!(selection.ordered(&paths), paths[..2]);

    selection.context_click(paths[2].clone());
    assert_eq!(selection.only(), Some(&paths[2]));
    selection.clear();
    assert_eq!(selection.len(), 0);
}
