use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Serialize, Clone)]
pub struct FileItem {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
    pub kind: String,
    pub extension: String,
    pub is_archive: bool,
}

#[derive(Serialize, Clone)]
pub struct DirectoryResult {
    pub current_path: String,
    pub files: Vec<FileItem>,
    pub is_archive: bool,
}

pub fn read_directory(dir_path: String) -> Result<DirectoryResult, String> {
    let absolute_path = if dir_path == "." {
        fs::canonicalize(".").map_err(|e| format!("Failed to get current directory: {}", e))?
    } else {
        let path = Path::new(&dir_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            fs::canonicalize(path).map_err(|e| format!("Failed to resolve path: {}", e))?
        }
    };

    let path_str = absolute_path.to_string_lossy().to_string();

    if !absolute_path.exists() {
        return Err("Directory does not exist".to_string());
    }

    if !absolute_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let entries =
        fs::read_dir(&absolute_path).map_err(|e| format!("Failed to read directory: {}", e))?;

    let mut files = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to read metadata: {}", e))?;

        let file_path = entry.path();
        let absolute_file_path = fs::canonicalize(&file_path).unwrap_or_else(|_| file_path.clone());
        let name = entry.file_name().to_string_lossy().to_string();

        let extension = if metadata.is_dir() {
            String::new()
        } else {
            file_path
                .extension()
                .map(|ext| ext.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        let ctime = metadata
            .created()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
            .unwrap_or(0);

        let mtime = metadata
            .modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
            .unwrap_or(0);

        let is_dir = metadata.is_dir();
        let file_item = FileItem {
            path: absolute_file_path.to_string_lossy().to_string(),
            is_archive: is_archive(&name, is_dir),
            name,
            size: metadata.len(),
            ctime,
            mtime,
            kind: get_file_kind(&extension, is_dir),
            extension,
        };

        files.push(file_item);
    }

    Ok(DirectoryResult {
        current_path: path_str,
        files: files
            .into_iter()
            .filter(|f| !f.name.starts_with('.'))
            .collect(),
        is_archive: false,
    })
}

static AUDIO: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "aiff", "alac",
    ]
    .into_iter()
    .collect()
});

static VIDEO: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v", "3gp", "mpeg", "mpg",
    ]
    .into_iter()
    .collect()
});

static IMAGE: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "tiff", "tif", "ico", "heic",
    ]
    .into_iter()
    .collect()
});

static TEXT: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "txt", "md", "csv", "json", "jsonl", "json5", "xml", "html", "htm", "css", "js", "jsx",
        "ts", "tsx", "py", "rs", "go", "java", "c", "cpp", "h", "hpp", "sh", "bat", "ps1", "ini",
        "yml", "yaml", "toml", "log", "sql", "env",
    ]
    .into_iter()
    .collect()
});

static COMPRESS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "tbz2", "txz", "zst",
    ]
    .into_iter()
    .collect()
});

pub fn get_file_kind(extension: &str, is_dir: bool) -> String {
    if is_dir {
        "folder"
    } else if AUDIO.contains(&extension) {
        "audio"
    } else if VIDEO.contains(&extension) {
        "video"
    } else if IMAGE.contains(&extension) {
        "image"
    } else if TEXT.contains(&extension) {
        "text"
    } else if COMPRESS.contains(&extension) {
        "compress"
    } else {
        "other"
    }
    .to_string()
}

static ARCHIVE_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "zip", "7z", "tar", "tar.gz", "tar.bz2", "tar.xz", "tgz", "tbz2", "txz", "rar",
    ]
});

fn is_archive(name: &str, is_dir: bool) -> bool {
    if is_dir {
        return false;
    }
    for extension in ARCHIVE_EXTENSIONS.clone().into_iter() {
        if name.ends_with(extension) {
            return true;
        }
    }
    false
}
