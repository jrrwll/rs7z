use crate::compress::ArchiveError;
use std::path::{Path, PathBuf};

pub fn list_zip(path: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        files.push(file.name().into());
    }
    Ok(files)
}

pub fn extract_zip(src: &Path, dest: &Path, entry_name: Option<&str>) -> Result<(), ArchiveError> {
    let file = std::fs::File::open(src)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let target_path = entry_name.map(Path::new);
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;

        let file_path = Path::new(file.name());
        if let Some(target) = target_path {
            // 只处理：完全匹配 或 以 target/ 开头（即子路径）
            if file_path != target && !file_path.starts_with(target) {
                continue;
            }
        }

        let out_path = dest.join(file.name());
        if file.name().ends_with('/') {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut outfile = std::fs::File::create(&out_path)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // keep file permissions for unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}
