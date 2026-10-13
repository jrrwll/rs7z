use std::{collections::BTreeSet, path::PathBuf};

#[derive(Default)]
pub(super) struct Selection {
    paths: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    cursor: Option<PathBuf>,
}

impl Selection {
    pub(super) fn clear(&mut self) {
        self.paths.clear();
        self.anchor = None;
        self.cursor = None;
    }

    pub(super) fn len(&self) -> usize {
        self.paths.len()
    }

    pub(super) fn cursor(&self) -> Option<&PathBuf> {
        self.cursor.as_ref()
    }

    pub(super) fn contains(&self, path: &PathBuf) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn only(&self) -> Option<&PathBuf> {
        (self.paths.len() == 1)
            .then(|| self.paths.iter().next())
            .flatten()
    }

    pub(super) fn ordered(&self, visible: &[PathBuf]) -> Vec<PathBuf> {
        visible
            .iter()
            .filter(|path| self.contains(path))
            .cloned()
            .collect()
    }

    pub(super) fn click(
        &mut self,
        visible: &[PathBuf],
        path: PathBuf,
        additive: bool,
        range: bool,
    ) {
        if range {
            self.range(visible, path);
        } else if additive {
            if !self.paths.insert(path.clone()) {
                self.paths.remove(&path);
            }
            self.anchor = Some(path.clone());
            self.cursor = Some(path);
        } else {
            self.single(path);
        }
    }

    pub(super) fn context_click(&mut self, path: PathBuf) {
        if !self.paths.contains(&path) {
            self.single(path);
        }
    }

    pub(super) fn step(&mut self, visible: &[PathBuf], down: bool, extend: bool) {
        if visible.is_empty() {
            return;
        }
        let position = self
            .cursor
            .as_ref()
            .and_then(|cursor| visible.iter().position(|path| path == cursor));
        let next = match position {
            Some(index) if down => (index + 1).min(visible.len() - 1),
            Some(index) => index.saturating_sub(1),
            None => 0,
        };
        let path = visible[next].clone();
        if extend {
            self.range(visible, path);
        } else {
            self.single(path);
        }
    }

    pub(super) fn select_all(&mut self, visible: &[PathBuf]) {
        self.paths = visible.iter().cloned().collect();
        self.anchor = visible.first().cloned();
        self.cursor = visible.last().cloned();
    }

    fn single(&mut self, path: PathBuf) {
        self.paths.clear();
        self.paths.insert(path.clone());
        self.anchor = Some(path.clone());
        self.cursor = Some(path);
    }

    fn range(&mut self, visible: &[PathBuf], path: PathBuf) {
        let anchor = self
            .anchor
            .as_ref()
            .and_then(|anchor| visible.iter().position(|item| item == anchor));
        let target = visible.iter().position(|item| item == &path);
        let (Some(anchor), Some(target)) = (anchor, target) else {
            self.single(path);
            return;
        };
        self.paths = visible[anchor.min(target)..=anchor.max(target)]
            .iter()
            .cloned()
            .collect();
        self.cursor = Some(path);
    }
}
