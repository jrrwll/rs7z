use crate::model::file::{FileItem, FileKind};
use chrono::{DateTime, Local};
use std::{fs, path::Path, time::SystemTime};

pub fn read_item(path: &Path) -> std::io::Result<FileItem> {
    let metadata = fs::metadata(path).or_else(|_| fs::symlink_metadata(path))?;
    let kind = if metadata.is_dir() {
        FileKind::Directory
    } else if metadata.is_file() && super::archive::format(path).is_some() {
        FileKind::Archive
    } else {
        FileKind::Other
    };
    Ok(FileItem {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        path: path.to_owned(),
        kind,
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

pub fn read_directory(path: &Path) -> std::io::Result<Vec<FileItem>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(metadata) = entry.metadata().or_else(|_| fs::symlink_metadata(&path)) else {
            continue;
        };
        let kind = if metadata.is_dir() {
            FileKind::Directory
        } else if metadata.is_file() && super::archive::format(&path).is_some() {
            FileKind::Archive
        } else {
            FileKind::Other
        };
        items.push(FileItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            kind,
            size: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    items.sort_by(|a, b| {
        let rank = |kind| match kind {
            FileKind::Directory => 0,
            FileKind::Archive => 1,
            FileKind::Other => 2,
        };
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(items)
}

pub fn format_modified(modified: Option<SystemTime>) -> String {
    format_modified_with(modified, "%Y-%m-%d %H:%M")
}

pub fn format_modified_seconds(modified: Option<SystemTime>) -> String {
    format_modified_with(modified, "%Y-%m-%d %H:%M:%S")
}

fn format_modified_with(modified: Option<SystemTime>, pattern: &str) -> String {
    match modified {
        Some(time) => DateTime::<Local>::from(time).format(pattern).to_string(),
        None => "--".to_owned(),
    }
}

pub fn format_info_size(bytes: u64) -> String {
    match size_unit(bytes) {
        None => format!("{bytes} bytes"),
        Some((unit_bytes, unit)) if bytes.is_multiple_of(unit_bytes) => {
            format!("{bytes} ({} {unit})", bytes / unit_bytes)
        }
        Some(_) => format!("{bytes} ({})", format_size(bytes)),
    }
}

pub fn format_size(bytes: u64) -> String {
    match size_unit(bytes) {
        Some((unit_bytes, unit)) => format!("{:.1} {unit}", bytes as f64 / unit_bytes as f64),
        None => format!("{bytes} bytes"),
    }
}

fn size_unit(bytes: u64) -> Option<(u64, &'static str)> {
    if bytes < 1024 {
        None
    } else if bytes < 1024 * 1024 {
        Some((1024, "KB"))
    } else if bytes < 1024 * 1024 * 1024 {
        Some((1024 * 1024, "MB"))
    } else {
        Some((1024 * 1024 * 1024, "GB"))
    }
}
