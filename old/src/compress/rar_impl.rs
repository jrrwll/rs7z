use crate::compress::ArchiveError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn list_rar(path: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut archive = unrar::Archive::new(path).open_for_listing()?;
    let mut files = Vec::new();

    while let Some(entry) = archive.read_header()? {
        files.push(entry.entry().filename.to_path_buf());
        archive = entry.skip()?;
    }

    Ok(files)
}

pub fn extract_entry_rar(
    path: &Path,
    entry_name: &str,
    output_dir: &Path,
) -> Result<(), ArchiveError> {
    let mut archive = unrar::Archive::new(path).open_for_processing()?;

    while let Some(header) = archive.read_header()? {
        let entry = header.entry();
        let archive_path = entry.filename.to_string_lossy().to_string();

        if archive_path == entry_name {
            let output_path = output_dir.join(&archive_path);

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if entry.is_directory() {
                fs::create_dir_all(&output_path)?;
                archive = header.skip()?;
            } else {
                let (data, new_archive) = header.read()?;
                let mut file = fs::File::create(&output_path)?;
                file.write_all(&data)?;
                archive = new_archive;
            }
        } else {
            // 跳过非目标条目
            archive = header.skip()?;
        }
    }
    Ok(())
}
