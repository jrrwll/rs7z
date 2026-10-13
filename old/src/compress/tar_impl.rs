use crate::compress::ArchiveError;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn list_tar(path: &Path, compression: Option<&str>) -> Result<Vec<PathBuf>, ArchiveError> {
    let file = std::fs::File::open(path)?;
    let buf_reader = std::io::BufReader::new(file);
    let decompressed: Box<dyn std::io::Read> = match compression {
        Some("gz") => Box::new(flate2::read::GzDecoder::new(buf_reader)),
        Some("bz2") => Box::new(bzip2::read::BzDecoder::new(buf_reader)),
        Some("xz") => Box::new(xz2::read::XzDecoder::new(buf_reader)),
        Some("zst") => Box::new(zstd::stream::read::Decoder::new(buf_reader)?),
        Some("br") => Box::new(brotli::Decompressor::new(buf_reader, 4096)),
        _ => Box::new(buf_reader),
    };
    let mut archive = tar::Archive::new(decompressed);
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let entry = entry.map_err(|_| ArchiveError::Tar)?;
        files.push(entry.path()?.to_path_buf());
    }
    Ok(files)
}

pub fn extract_tar(
    src: &Path,
    dest: &Path,
    compression: Option<&str>,
    entry_name: Option<&str>,
) -> Result<(), ArchiveError> {
    let file = std::fs::File::open(src)?;
    let buf_reader = std::io::BufReader::new(file);
    let decompressed: Box<dyn std::io::Read> = match compression {
        Some("gz") => Box::new(flate2::read::GzDecoder::new(buf_reader)),
        Some("bz2") => Box::new(bzip2::read::BzDecoder::new(buf_reader)),
        Some("xz") => Box::new(xz2::read::XzDecoder::new(buf_reader)),
        Some("zst") => Box::new(zstd::stream::read::Decoder::new(buf_reader)?),
        Some("br") => Box::new(brotli::Decompressor::new(buf_reader, 4096)),
        _ => Box::new(buf_reader),
    };
    let mut archive = tar::Archive::new(decompressed);

    let Some(target_path) = entry_name.map(Path::new) else {
        archive.unpack(dest)?;
        return Ok(());
    };

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let path_in_archive = path.as_ref();

        // 跳过不匹配的条目
        if path_in_archive != target_path && !path_in_archive.starts_with(target_path) {
            continue;
        }

        // 构造输出路径
        let out_path = dest.join(path_in_archive);
        if !out_path.starts_with(dest) {
            return Err(ArchiveError::InvalidPath(path_in_archive.to_owned()));
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 写入文件或创建目录
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            let mut outfile = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode()?;
            std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

pub fn is_tar_gz(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut header = [0; 261];
        if decoder.read_exact(&mut header).is_ok() {
            return tar::Archive::new(std::io::Cursor::new(&header))
                .entries()
                .is_ok();
        }
    }
    false
}

pub fn is_tar_bz2(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        let mut decoder = bzip2::read::BzDecoder::new(file);
        let mut header = [0; 261];
        if decoder.read_exact(&mut header).is_ok() {
            return tar::Archive::new(std::io::Cursor::new(&header))
                .entries()
                .is_ok();
        }
    }
    false
}

pub fn is_tar_xz(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        let mut decoder = xz2::read::XzDecoder::new(file);
        let mut header = [0; 261];
        if decoder.read_exact(&mut header).is_ok() {
            return tar::Archive::new(std::io::Cursor::new(&header))
                .entries()
                .is_ok();
        }
    }
    false
}

pub fn is_tar_zst(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        let mut decoder = zstd::Decoder::new(file).unwrap();
        let mut header = [0; 261];
        if decoder.read_exact(&mut header).is_ok() {
            return tar::Archive::new(std::io::Cursor::new(&header))
                .entries()
                .is_ok();
        }
    }
    false
}

pub fn is_tar_br(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        let mut decoder = brotli::Decompressor::new(file, 4096);
        let mut header = [0; 261];
        if decoder.read_exact(&mut header).is_ok() {
            return tar::Archive::new(std::io::Cursor::new(&header))
                .entries()
                .is_ok();
        }
    }
    false
}
