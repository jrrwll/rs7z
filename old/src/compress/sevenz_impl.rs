use crate::compress::ArchiveError;

use sevenz_rust2::{decompress_file, ArchiveReader, Password};
use std::fs;
use std::path::{Path, PathBuf};

pub fn list_7z(path: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let file = fs::File::open(path)?;
    let mut reader = ArchiveReader::new(file, Password::empty())?;

    let mut files = Vec::new();

    reader.for_each_entries(|entry, _| {
        files.push(PathBuf::from(entry.name.clone()));
        Ok(true)
    })?;
    Ok(files)
}

pub fn extract_7z_entry(
    src: &Path,
    dest: &Path,
    entry_name: Option<&str>,
) -> Result<(), ArchiveError> {
    fs::create_dir_all(dest)?;

    let file = fs::File::open(src)?;
    let mut reader = ArchiveReader::new(file, Password::empty())?;

    let Some(target_path) = entry_name.map(Path::new) else {
        decompress_file(src, dest)?;
        return Ok(());
    };

    reader.for_each_entries(|entry, mut r| {
        let current = Path::new(&entry.name);

        if current == target_path || current.starts_with(target_path) {
            let out_path = dest.join(current);

            if entry.is_directory() {
                fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&out_path)?;
                std::io::copy(&mut r, &mut outfile)?;
            }
        } else {
            // skip, also need consume
            std::io::copy(&mut r, &mut std::io::sink())?;
        }
        Ok(true)
    })?;
    Ok(())
}
