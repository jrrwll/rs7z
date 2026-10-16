use super::{ProgressHandle, extract_using_command};
use crate::model::{operations::OperationError, zip::ZipOptions};
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn fixture(path: &Path) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    for name in ["damaged.txt", "keep.txt"] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contents").unwrap();
    }
    zip.finish().unwrap();
}

fn command(destination: &Path, diagnostic: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(format!(
        "mkdir -p \"$1\"; printf damaged > \"$1/damaged.txt\"; printf keep > \"$1/keep.txt\"; {diagnostic}"
    )).arg("extract-fixture").arg(destination);
    command
}

#[test]
fn startup_failure_removes_only_new_empty_directories() {
    let root = tempfile::tempdir().unwrap();
    // An unsupported extension skips metadata so this checks spawn failure directly.
    let source = root.path().join("source.unknown");
    fs::write(&source, b"unused").unwrap();
    for existing in [false, true] {
        let parent = root.path().join(existing.to_string());
        let destination = parent.join("nested/output");
        if existing {
            fs::create_dir_all(&destination).unwrap();
        }
        let error = extract_using_command(
            &source,
            &destination,
            &ZipOptions::default(),
            &ProgressHandle::default(),
            Command::new(root.path().join("missing-7zz")),
        )
        .unwrap_err();
        assert!(
            matches!(error, OperationError::Failed(message) if message.contains("Cannot start 7zz"))
        );
        if existing {
            assert!(destination.is_dir());
            assert!(fs::read_dir(&destination).unwrap().next().is_none());
        } else {
            assert!(!parent.exists());
        }
    }
}

#[test]
fn write_errors_unknown_errors_and_abnormal_exits_preserve_output() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.zip");
    fixture(&source);
    for (number, diagnostic) in [
        "printf 'ERROR: CRC Failed : damaged.txt\\nERROR: Write Error : No space left on device\\n'; exit 2",
        "printf 'ERROR: CRC Failed : damaged.txt\\nSystem ERROR: Permission denied\\n'; exit 2",
        "printf 'ERROR: CRC Failed : damaged.txt\\n'; printf 'Write failed: No space left on device\\n' >&2; exit 2",
        "printf 'ERROR: CRC Failed : damaged.txt\\nERROR: Unrecognized failure\\n'; exit 2",
        "printf 'ERROR: CRC Failed : damaged.txt\\n'; kill -TERM $$",
        "printf 'ERROR: CRC Failed : damaged.txt\\n'; exit 7",
        "exit 2",
        // The reported path must exactly match an indexed archive entry.
        "printf 'ERROR: CRC Failed : ../damaged.txt\\n'; exit 2",
    ].iter().enumerate() {
        let destination = root.path().join(number.to_string());
        assert!(extract_using_command(
            &source, &destination, &ZipOptions::default(), &ProgressHandle::default(),
            command(&destination, diagnostic),
        ).is_err());
        assert_eq!(fs::read(destination.join("damaged.txt")).unwrap(), b"damaged", "{diagnostic}");
        assert_eq!(fs::read(destination.join("keep.txt")).unwrap(), b"keep");
    }
}

#[test]
fn cleanup_matches_the_complete_entry_name_even_after_long_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.zip");
    let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
    for name in ["damaged.txt", "damaged.txt ", "keep.txt"] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contents").unwrap();
    }
    zip.finish().unwrap();
    let destination = root.path().join("output");
    let diagnostic = "printf partial > \"$1/damaged.txt \"; i=0; while [ \"$i\" -lt 500 ]; do printf 'long output line\\n'; i=$((i+1)); done; printf 'ERROR: CRC Failed : damaged.txt ' ; exit 2";
    assert!(
        extract_using_command(
            &source,
            &destination,
            &ZipOptions::default(),
            &ProgressHandle::default(),
            command(&destination, diagnostic),
        )
        .is_err()
    );
    assert!(!destination.join("damaged.txt ").exists());
    assert_eq!(
        fs::read(destination.join("damaged.txt")).unwrap(),
        b"damaged"
    );
    assert_eq!(fs::read(destination.join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn cancellation_after_a_corrupt_entry_preserves_all_partial_results() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.zip");
    fixture(&source);
    let destination = root.path().join("output");
    let worker_destination = destination.clone();
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        extract_using_command(
            &source,
            &worker_destination,
            &ZipOptions::default(),
            &worker_progress,
            command(
                &worker_destination,
                "printf 'ERROR: CRC Failed : damaged.txt\\n'; while :; do :; done",
            ),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !destination.join("keep.txt").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(
        fs::read(destination.join("damaged.txt")).unwrap(),
        b"damaged"
    );
    assert_eq!(fs::read(destination.join("keep.txt")).unwrap(), b"keep");
}
