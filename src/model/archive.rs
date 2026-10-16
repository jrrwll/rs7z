use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Tar,
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
    Fast,
    Normal,
    Maximum,
    Ultra,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZipEncryption {
    #[default]
    ZipCrypt,
    Aes256,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TarFormat {
    #[default]
    Gnu,
    Posix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SevenZipMethod {
    Lzma2,
    Lzma,
    Ppmd,
    Bzip2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidBlockSize {
    NonSolid,
    Size(u64),
    Solid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SevenZipOptions {
    pub method: Option<SevenZipMethod>,
    pub dictionary_size: Option<u64>,
    pub word_size: Option<u16>,
    pub solid_block_size: Option<SolidBlockSize>,
}

#[derive(Clone, Default)]
#[non_exhaustive]
pub struct ArchiveOptions {
    pub password: Option<String>,
    pub zip_encryption: ZipEncryption,
    pub compression_level: Option<CompressionLevel>,
    pub tar_format: TarFormat,
    pub seven_zip: SevenZipOptions,
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
            .field("tar_format", &self.tar_format)
            .field("seven_zip", &self.seven_zip)
            .field("zip_encryption", &self.zip_encryption)
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
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub metadata: ArchiveEntryMetadata,
}

/// Optional properties recorded in the archive, never computed from extracted data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ArchiveEntryMetadata {
    pub compressed_size: Option<u64>,
    pub encrypted: Option<bool>,
    pub comment: Option<String>,
    pub crc32: Option<u32>,
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
