use super::*;

#[cfg(unix)]
#[test]
fn cancellation_keeps_binary_partial_and_reports_its_path() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.gz");
    fs::write(&source, b"fixture").unwrap();
    let destination = root.path().join("output");
    let output_dir = destination.clone();
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "printf 'decoded bytes'; printf ' 42%% + source' >&2; while :; do sleep 0.01; done",
        "fixture",
    ]);
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        extract_using_command(
            &source,
            &destination,
            StreamFormat::Gzip,
            &worker_progress,
            command,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress.snapshot().percent != 42. && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let observed = progress.snapshot().percent;
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(observed, 42.);
    assert!(!output_dir.join("source").exists());
    let partial = fs::read_dir(&output_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(partial.extension().unwrap(), "partial");
    assert_eq!(fs::read(&partial).unwrap(), b"decoded bytes");
    assert!(
        progress
            .snapshot()
            .item
            .ends_with(partial.to_str().unwrap())
    );
}

#[cfg(unix)]
#[test]
fn publication_race_preserves_existing_file_and_complete_partial() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.gz");
    fs::write(&source, b"fixture").unwrap();
    let output = root.path().join("source");
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "printf 'decoded bytes'; printf 'other process' > \"$STREAM_OUTPUT\"",
        "fixture",
    ]);
    command.env("STREAM_OUTPUT", &output);
    let error = extract_using_command(
        &source,
        root.path(),
        StreamFormat::Gzip,
        &ProgressHandle::default(),
        command,
    )
    .unwrap_err();
    assert!(
        matches!(error, OperationError::Failed(ref text) if text.contains("Partial output retained at:"))
    );
    assert_eq!(fs::read(output).unwrap(), b"other process");
    let partial = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "partial")
        })
        .unwrap();
    assert_eq!(fs::read(partial).unwrap(), b"decoded bytes");
}
