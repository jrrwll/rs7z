use rs7z::service::archive;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// Default to home; resolve explicit relative paths against the process directory.
pub(super) fn path(
    arguments: impl IntoIterator<Item = OsString>,
    directory: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let mut arguments = arguments.into_iter();
    let argument = arguments.next();
    if arguments.next().is_some() {
        return Err("Expected at most one directory or browsable archive path.".into());
    }
    let path = match argument {
        Some(path) => PathBuf::from(path),
        None => home
            .ok_or("Cannot determine the user's home directory. Pass a path explicitly.")?
            .to_owned(),
    };
    if path.as_os_str().is_empty() {
        return Err("The path must not be empty.".into());
    }
    let path = if path.is_absolute() {
        path
    } else {
        directory.join(path)
    };
    let metadata =
        fs::metadata(&path).map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    if metadata.is_dir()
        || (metadata.is_file()
            && archive::format(&path).is_some()
            && !archive::is_extract_only(&path))
    {
        Ok(path)
    } else {
        Err(format!(
            "Cannot browse {}. Choose a directory or a browsable archive; RAR and standalone streams support extraction only.",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::path;
    use std::{ffi::OsString, fs, path::Path};

    #[test]
    fn optional_path_defaults_to_home_and_resolves_relative_paths_against_working_directory() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        let directory = root.path().join("文件 with spaces");
        fs::create_dir(&directory).unwrap();
        assert_eq!(path([], root.path(), Some(&home)).unwrap(), home);
        assert_eq!(path([], Path::new("/"), Some(&home)).unwrap(), home);
        assert_eq!(
            path([OsString::from(".")], root.path(), Some(&home)).unwrap(),
            root.path().join(".")
        );
        assert_eq!(
            path(
                [OsString::from("文件 with spaces")],
                root.path(),
                Some(&home)
            )
            .unwrap(),
            directory
        );
        assert_eq!(
            path([directory.into_os_string()], root.path(), Some(&home)).unwrap(),
            root.path().join("文件 with spaces")
        );
    }

    #[test]
    fn missing_home_requires_an_explicit_path() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            path([], root.path(), None).unwrap_err(),
            "Cannot determine the user's home directory. Pass a path explicitly."
        );
        assert!(path([OsString::from(".")], root.path(), None).is_ok());
        assert!(path([root.path().as_os_str().to_owned()], root.path(), None).is_ok());
    }

    #[test]
    fn only_existing_directories_and_browsable_archive_files_are_accepted() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "a.zip",
            "a.7z",
            "a.tar",
            "a.tgz",
            "a.tar.Z",
            "a.zip.001",
            "a.7z.001",
        ] {
            fs::write(root.path().join(name), b"fixture").unwrap();
            assert!(path([OsString::from(name)], root.path(), None).is_ok());
        }
        for name in [
            "a.txt", "a.rar", "a.gz", "a.bz2", "a.xz", "a.zstd", "a.lzma", "a.Z", "a.lzma86",
            "a.b64", "missing",
        ] {
            if name != "missing" {
                fs::write(root.path().join(name), b"fixture").unwrap();
            }
            assert!(
                path([OsString::from(name)], root.path(), None).is_err(),
                "{name}"
            );
        }
        fs::create_dir(root.path().join("folder.gz")).unwrap();
        assert!(path([OsString::from("folder.gz")], root.path(), None).is_ok());
        assert!(path([OsString::new()], root.path(), None).is_err());
        assert!(
            path(
                [OsString::from("."), OsString::from("extra")],
                root.path(),
                None
            )
            .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_paths_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let name = OsString::from_vec(b"directory-\xff".to_vec());
        fs::create_dir(root.path().join(&name)).unwrap();
        assert_eq!(
            path([name.clone()], root.path(), None).unwrap(),
            root.path().join(name)
        );
    }
}
