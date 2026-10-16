use super::{PROGRAM, resolve};
use std::{env, fs, path::Path};

#[test]
fn rar_support_requires_decoders_not_just_readable_headers() {
    let formats = "Formats:\n Rar\n Rar5\nCodecs:\n ED Copy\nHashers:\n CRC32\n";
    assert!(!super::has_rar_decoders(formats));
    let complete =
        "Codecs:\n D 40301 Rar1\n D 40302 Rar2\n D 40303 Rar3\n D 40305 Rar5\nHashers:\n";
    assert!(super::has_rar_decoders(complete));
    assert!(!super::has_rar_decoders(
        &complete.replace("D 40305 Rar5", "E 40305 Rar5")
    ));
}

fn program(directory: &Path, name: &str) -> std::path::PathBuf {
    fs::create_dir_all(directory).unwrap();
    let path = directory.join(name);
    fs::write(&path, b"test executable").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::canonicalize(path).unwrap()
}

#[test]
fn explicit_override_takes_precedence_and_invalid_override_does_not_fall_back() {
    let root = tempfile::tempdir().unwrap();
    let custom = program(&root.path().join("custom directory"), PROGRAM);
    let bundled = program(&root.path().join("app"), PROGRAM);
    let app = bundled.parent().unwrap().join("rs7z");
    assert_eq!(
        resolve(Some(custom.as_os_str()), Some(&app), None, &[]).unwrap(),
        custom
    );
    let missing = root.path().join("missing");
    let error = resolve(Some(missing.as_os_str()), Some(&app), None, &[]).unwrap_err();
    assert!(error.contains("RS7Z_7ZZ is not an executable file"));
    assert!(error.contains("missing"));
    for path in [Path::new(""), Path::new("7zz"), Path::new("./7zz")] {
        assert!(
            resolve(Some(path.as_os_str()), Some(&app), None, &[])
                .unwrap_err()
                .contains("absolute path")
        );
    }
}

#[test]
fn bundled_cli_precedes_path_and_path_precedes_platform_fallback() {
    let root = tempfile::tempdir().unwrap();
    let bundled = program(&root.path().join("App.app/Contents/MacOS"), PROGRAM);
    let from_path = program(&root.path().join("bin"), PROGRAM);
    let fallback = program(&root.path().join("homebrew/bin"), PROGRAM);
    let app = bundled.parent().unwrap().join("rs7z");
    let path = env::join_paths([from_path.parent().unwrap()]).unwrap();
    let fallback = [fallback.parent().unwrap()];
    assert_eq!(
        resolve(None, Some(&app), Some(&path), &fallback).unwrap(),
        bundled
    );
    assert_eq!(
        resolve(None, None, Some(&path), &fallback).unwrap(),
        from_path
    );
}

#[test]
fn finder_launch_without_homebrew_on_path_uses_platform_fallback() {
    let root = tempfile::tempdir().unwrap();
    let expected = program(&root.path().join("homebrew/bin"), PROGRAM);
    let path = env::join_paths([root.path().join("system/bin")]).unwrap();
    assert_eq!(
        resolve(None, None, Some(&path), &[expected.parent().unwrap()]).unwrap(),
        expected
    );
}

#[test]
fn legacy_7z_only_installation_reports_how_to_install_or_configure_7zz() {
    let root = tempfile::tempdir().unwrap();
    program(root.path(), "7z");
    program(root.path(), "7za");
    let path = env::join_paths([root.path()]).unwrap();
    let error = resolve(None, None, Some(&path), &[]).unwrap_err();
    assert!(error.contains("Cannot find the official 7-Zip CLI (7zz)"));
    assert!(error.contains("https://7-zip.org/download.html"));
    assert!(error.contains("RS7Z_7ZZ"));
}

#[cfg(unix)]
#[test]
fn path_skips_nonexecutables_and_resolves_symlinks_before_changing_working_directory() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let root = tempfile::tempdir().unwrap();
    let blocked = program(&root.path().join("blocked"), PROGRAM);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o644)).unwrap();
    let target = program(&root.path().join("cellar/sevenzip/bin"), PROGRAM);
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    symlink(&target, bin.join(PROGRAM)).unwrap();
    let path = env::join_paths([blocked.parent().unwrap(), &bin]).unwrap();
    assert_eq!(resolve(None, None, Some(&path), &[]).unwrap(), target);
    assert!(resolve(Some(blocked.as_os_str()), None, Some(&path), &[]).is_err());
}
