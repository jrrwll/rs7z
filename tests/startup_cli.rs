use std::{fs, process::Command};

#[test]
fn invalid_startup_arguments_exit_before_opening_a_window() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("data.gz"), b"fixture").unwrap();
    fs::write(root.path().join("data.txt"), b"fixture").unwrap();
    for (args, expected) in [
        (vec!["missing"], "Cannot open"),
        (vec!["data.gz"], "Cannot browse"),
        (vec!["data.txt"], "Cannot browse"),
        (vec![".", "extra"], "Expected at most one"),
        (vec![""], "must not be empty"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rs7z"))
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("Usage: rs7z [path]"));
    }
}
