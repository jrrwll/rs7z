use crate::file::{DirectoryResult, FileItem};
pub use archive::*;
pub use base::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

mod archive;
mod base;
mod rar_impl;
mod sevenz_impl;
mod tar_impl;
mod zip_impl;

static ARCHIVE_CACHE: LazyLock<Mutex<HashMap<String, Vec<TreeNode>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn list_archive_contents(archive_path: String) -> Result<DirectoryResult, String> {
    // 检查缓存
    {
        let cache = ARCHIVE_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&archive_path) {
            let files = convert_tree_nodes_to_file_items(&archive_path, "", cached);
            return Ok(DirectoryResult {
                current_path: archive_path.clone(),
                files,
                is_archive: true,
            });
        }
    }

    // 调用 list_archive
    let path = Path::new(&archive_path);
    let tree_nodes = list_archive(path).map_err(|e| format!("Failed to list archive: {}", e))?;

    // 缓存结果
    {
        let mut cache = ARCHIVE_CACHE.lock().unwrap();
        cache.insert(archive_path.clone(), tree_nodes.clone());
    }

    let files = convert_tree_nodes_to_file_items(&archive_path, "", &tree_nodes);

    Ok(DirectoryResult {
        current_path: archive_path,
        files,
        is_archive: true,
    })
}

pub fn list_archive_path_contents(
    archive_path: String,
    archive_internal_path: String,
) -> Result<DirectoryResult, String> {
    // 检查缓存
    let tree_nodes = {
        let cache = ARCHIVE_CACHE.lock().unwrap();
        cache
            .get(&archive_path)
            .cloned()
            .unwrap_or_else(|| Vec::new())
    };

    if tree_nodes.is_empty() {
        return Err("Archive not cached. Please open archive first.".to_string());
    }

    // 过滤树，找到对应路径的节点
    let target_nodes = find_children_by_path(&tree_nodes, &archive_internal_path);

    let files =
        convert_tree_nodes_to_file_items(&archive_path, &archive_internal_path, &target_nodes);

    let current_path = if archive_internal_path.is_empty() {
        archive_path.clone()
    } else {
        format!("{}!{}", archive_path, archive_internal_path)
    };

    Ok(DirectoryResult {
        current_path,
        files,
        is_archive: true,
    })
}

fn find_children_by_path(nodes: &[TreeNode], path: &str) -> Vec<TreeNode> {
    if path.is_empty() {
        return nodes.to_vec();
    }

    let path_trimmed = path.trim_end_matches('/');

    // 找到目标节点
    let target_node = nodes.iter().find(|node| node.path == path_trimmed);

    match target_node {
        Some(node) if node.is_dir => node.children.clone(),
        Some(_) => Vec::new(),
        None => {
            // 如果在根节点找不到，尝试在子节点中找
            for node in nodes {
                if let Some(children) = find_recursive(node, &path_trimmed) {
                    return children;
                }
            }
            Vec::new()
        }
    }
}

fn find_recursive(node: &TreeNode, path: &str) -> Option<Vec<TreeNode>> {
    if node.path == path {
        return if node.is_dir {
            Some(node.children.clone())
        } else {
            None
        };
    }

    for child in &node.children {
        if let Some(result) = find_recursive(child, path) {
            return Some(result);
        }
    }

    None
}

fn convert_tree_nodes_to_file_items(
    archive_path: &str,
    archive_internal_path: &str,
    nodes: &[TreeNode],
) -> Vec<FileItem> {
    nodes
        .iter()
        .filter(|node| !node.name.starts_with('.'))
        .map(|node| {
            let internal_path = if archive_internal_path.is_empty() {
                node.path.clone()
            } else {
                node.path.clone()
            };
            let full_path = format!("{}!{}", archive_path, internal_path);
            let extension = get_extension_from_path(&node.name);
            let kind = crate::file::get_file_kind(&extension, node.is_dir);

            FileItem {
                path: full_path,
                name: node.name.clone(),
                size: 0,
                ctime: 0,
                mtime: 0,
                kind,
                extension,
                is_archive: false,
            }
        })
        .collect()
}

fn get_extension_from_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let path_obj = Path::new(path);
    path_obj
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
}
