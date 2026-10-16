use std::{path::PathBuf, time::SystemTime};

use super::archive::ArchiveEntryMetadata;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    Archive,
    Other,
}

#[derive(Clone, Debug)]
pub struct FileItem {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub archive: Option<ArchiveEntryMetadata>,
}
