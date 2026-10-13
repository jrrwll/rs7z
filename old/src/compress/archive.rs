use crate::compress::sevenz_impl::{extract_7z_entry, list_7z};
use crate::compress::tar_impl::{
    extract_tar, is_tar_br, is_tar_bz2, is_tar_gz, is_tar_xz, is_tar_zst, list_tar,
};
use crate::compress::zip_impl::{extract_zip, list_zip};
use crate::compress::{build_tree_from_paths, ArchiveError, TreeNode};
use std::path::Path;

pub fn list_archive(archive_path: &Path) -> anyhow::Result<Vec<TreeNode>, ArchiveError> {
    let ext = get_extension(archive_path);
    let paths = match ext.as_str() {
        "zip" => list_zip(archive_path),
        "tar" => list_tar(archive_path, None),
        "gz" if is_tar_gz(archive_path) => list_tar(archive_path, Some("gz")),
        "tgz" => list_tar(archive_path, Some("gz")),
        "bz2" if is_tar_bz2(archive_path) => list_tar(archive_path, Some("bz2")),
        "tbz2" => list_tar(archive_path, Some("bz2")),
        "xz" if is_tar_xz(archive_path) => list_tar(archive_path, Some("xz")),
        "txz" => list_tar(archive_path, Some("xz")),
        "zst" if is_tar_zst(archive_path) => list_tar(archive_path, Some("zst")),
        "br" if is_tar_br(archive_path) => list_tar(archive_path, Some("br")),
        "7z" => list_7z(archive_path),
        _ => Err(ArchiveError::UnsupportedFormat(ext)),
    }?;
    Ok(build_tree_from_paths(&paths))
}

pub fn extract_archive(
    archive_path: &Path,
    output_dir: &Path,
    entry_name: Option<&str>,
) -> anyhow::Result<(), ArchiveError> {
    std::fs::create_dir_all(output_dir)?;
    let ext = get_extension(archive_path);
    match ext.as_str() {
        "zip" => extract_zip(archive_path, output_dir, entry_name),
        "tar" => extract_tar(archive_path, output_dir, None, entry_name),
        "gz" if is_tar_gz(archive_path) => {
            extract_tar(archive_path, output_dir, Some("gz"), entry_name)
        }
        "tgz" => extract_tar(archive_path, output_dir, Some("gz"), entry_name),
        "bz2" if is_tar_bz2(archive_path) => {
            extract_tar(archive_path, output_dir, Some("bz2"), entry_name)
        }
        "tbz2" => extract_tar(archive_path, output_dir, Some("bz2"), entry_name),
        "xz" if is_tar_xz(archive_path) => {
            extract_tar(archive_path, output_dir, Some("xz"), entry_name)
        }
        "txz" => extract_tar(archive_path, output_dir, Some("xz"), entry_name),
        "zst" if is_tar_zst(archive_path) => {
            extract_tar(archive_path, output_dir, Some("zst"), entry_name)
        }
        "br" if is_tar_br(archive_path) => {
            extract_tar(archive_path, output_dir, Some("br"), entry_name)
        }
        "7z" => extract_7z_entry(archive_path, output_dir, entry_name),
        _ => Err(ArchiveError::UnsupportedFormat(ext)),
    }
}

fn get_extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}
