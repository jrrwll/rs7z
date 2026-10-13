use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArchiveError {
    #[error("Unsupported archive format: {0}")]
    UnsupportedFormat(String),
    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Tar error")]
    Tar,
    #[error("7z error: {0}")]
    SevenZ(#[from] sevenz_rust2::Error),
    #[error("rar error: {0}")]
    UnrarError(#[from] unrar::error::UnrarError),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TreeNode {
    pub name: String,            // 当前节点名（不含父路径）
    pub path: String,            // 在 ZIP 中的完整路径（如 "a/b/c.txt"）
    pub is_dir: bool,            // 是否为目录
    pub children: Vec<TreeNode>, // 子节点
}

impl TreeNode {
    fn new(name: String, path: String, is_dir: bool) -> Self {
        TreeNode {
            name,
            path,
            is_dir,
            children: Vec::new(),
        }
    }
}

pub fn build_tree_from_paths(paths: &[PathBuf]) -> Vec<TreeNode> {
    let mut root = TreeNode::new("".to_string(), "".to_string(), true);

    for entry_path in paths {
        // 转为字符串（ZIP 路径通常是 UTF-8）
        let path_str = match entry_path.to_str() {
            Some(s) => s,
            None => continue, // 跳过非 UTF-8 路径
        };
        // 路径标准化，去掉末尾 /
        let path_str = if path_str.ends_with('/') {
            &path_str[..path_str.len() - 1]
        } else {
            path_str
        };

        let components: Vec<&str> = entry_path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(os_str) => os_str.to_str(),
                _ => None,
            })
            .collect();

        if components.is_empty() {
            continue;
        }

        let mut current = &mut root;
        let mut current_prefix = String::new(); // 用于构建 path

        for (i, &comp) in components.iter().enumerate() {
            if !current_prefix.is_empty() {
                current_prefix.push('/');
            }
            current_prefix.push_str(comp);

            let is_last = i == components.len() - 1;
            // 判断是否为目录：要么是中间节点，要么原路径以 / 结尾
            let is_dir = !is_last || path_str.ends_with('/');

            // 查找是否已存在同名子节点
            let child_index = current.children.iter().position(|c| c.name == comp);
            if let Some(idx) = child_index {
                // 获取可变引用（只借用一次）
                let child = &mut current.children[idx];
                // 如果之前是文件，现在发现是目录，则升级
                if !child.is_dir && is_dir {
                    child.is_dir = true;
                }
                current = child;
            } else {
                let new_node = TreeNode::new(comp.to_string(), current_prefix.clone(), is_dir);
                current.children.push(new_node);
                // 安全地获取最后一个元素的可变引用
                current = current.children.last_mut().unwrap();
            }
        }
    }
    root.children
}
