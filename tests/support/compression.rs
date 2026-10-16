#[cfg(unix)]
use super::run_stream;
use super::{Tool, find_executable, missing_message};
#[cfg(unix)]
use crate::model::operations::OperationError;
#[cfg(unix)]
use crate::service::operations::ProgressHandle;
#[cfg(unix)]
use std::process::Command;
use std::{env, fs};

#[test]
fn missing_tools_report_platform_specific_installation_commands() {
    for tool in [Tool::Xz, Tool::Zstd] {
        let name = tool.name();
        let message = missing_message(tool, "macos", "");
        assert!(message.contains(name));
        assert!(message.contains("brew install xz zstd"));
        assert!(message.contains("then try again"));
        let debian = if matches!(tool, Tool::Xz) {
            "xz-utils"
        } else {
            "zstd"
        };
        for release in ["ID=ubuntu", "ID=mint\nID_LIKE=\"ubuntu debian\""] {
            assert!(
                missing_message(tool, "linux", release)
                    .contains(&format!("sudo apt install {debian}"))
            );
        }
        assert!(
            missing_message(tool, "linux", "ID=fedora")
                .contains(&format!("sudo dnf install {name}"))
        );
        assert!(
            missing_message(tool, "linux", "ID=manjaro\nID_LIKE=arch")
                .contains(&format!("sudo pacman -S {name}"))
        );
        assert!(
            missing_message(tool, "linux", "ID=alpine").contains(&format!("sudo apk add {name}"))
        );
        assert!(
            missing_message(tool, "linux", "").contains("Choose the command for your distribution")
        );
        assert!(missing_message(tool, "windows", "").contains("scoop install xz zstd"));
    }
}

#[test]
fn codec_discovery_resolves_paths_and_handles_missing_tools_without_which() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin with spaces");
    fs::create_dir(&bin).unwrap();
    let path = env::join_paths([&bin]).unwrap();
    assert!(find_executable(Tool::Xz, Some(&path), &[]).is_none());
    let tool = bin.join(if cfg!(windows) { "xz.exe" } else { "xz" });
    fs::write(&tool, b"fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(find_executable(Tool::Xz, Some(&path), &[]).is_none());
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let expected = fs::canonicalize(&tool).unwrap();
    assert_eq!(
        find_executable(Tool::Xz, Some(&path), &[]),
        Some(expected.clone())
    );
    assert_eq!(find_executable(Tool::Xz, None, &[&bin]), Some(expected));
}

#[cfg(unix)]
#[test]
fn system_codec_failure_keeps_binary_output_and_reports_stderr() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("partial.tar");
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "printf 'binary data'; printf 'checksum mismatch\\n' >&2; exit 1",
    ]);
    let error = run_stream(command, Tool::Zstd, &output, &ProgressHandle::default()).unwrap_err();
    let OperationError::Failed(error) = error else {
        panic!("expected failure")
    };
    assert!(error.contains("zstd failed"));
    assert!(error.contains("checksum mismatch"));
    assert_eq!(fs::read(output).unwrap(), b"binary data");
}

#[cfg(unix)]
#[test]
fn system_codec_cancellation_stops_the_process_and_keeps_partial_output() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("partial.lzma");
    let worker_output = output.clone();
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'partial'; while :; do :; done"]);
        run_stream(command, Tool::Xz, &worker_output, &worker_progress)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::metadata(&output).map_or(true, |metadata| metadata.len() == 0)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(fs::read(&output).unwrap(), b"partial");
    assert!(progress.snapshot().percent < 100.);
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        run_stream(
            Command::new("/bin/sh"),
            Tool::Xz,
            &root.path().join("absent"),
            &cancelled
        ),
        Err(OperationError::Cancelled)
    );
    assert!(!root.path().join("absent").exists());
}
