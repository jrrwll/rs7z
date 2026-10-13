use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    TarGzip,
    TarBzip2,
    TarXz,
    TarLzma,
    TarZstd,
    TarCompress,
    Rar,
    Stream(StreamFormat),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFormat {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Lzma,
    Compress,
}

impl StreamFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gzip => "gz",
            Self::Bzip2 => "bz2",
            Self::Xz => "xz",
            Self::Zstd => "zst",
            Self::Lzma => "lzma",
            Self::Compress => "Z",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionLevel {
    Store,
    Fastest,
    Standard,
    Maximum,
}

#[derive(Clone, Default)]
#[non_exhaustive]
pub struct ArchiveOptions {
    pub password: Option<String>,
    pub compression_level: Option<CompressionLevel>,
    /// Volume size in bytes; supported when creating a new .zip or .7z archive.
    pub volume_size: Option<u64>,
}

impl std::fmt::Debug for ArchiveOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArchiveOptions")
            .field(
                "encrypted",
                &self
                    .password
                    .as_ref()
                    .is_some_and(|password| !password.is_empty()),
            )
            .field("compression_level", &self.compression_level)
            .field("volume_size", &self.volume_size)
            .finish()
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ArchiveEntry {
    pub name: String,
    pub directory: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ArchiveIndex {
    pub path: PathBuf,
    pub format: ArchiveFormat,
    pub encrypted: bool,
    pub read_only: bool,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Clone, Debug)]
pub enum ArchiveDestination {
    Directory(String),
    FileSystem(PathBuf),
}

#[derive(Clone, Debug)]
pub enum ArchiveOperation {
    Add {
        sources: Vec<PathBuf>,
        directory: String,
    },
    Delete {
        entries: Vec<String>,
    },
    Rename {
        entry: String,
        name: String,
    },
    Transfer {
        entries: Vec<String>,
        destination: ArchiveDestination,
        move_entries: bool,
    },
}
