use super::{ProgressHandle, SevenZipOutput};

#[test]
fn live_output_preserves_utf8_and_updates_unterminated_status_lines() {
    let progress = ProgressHandle::with_output();
    let mut parser = SevenZipOutput::default();
    let text = "Extracting archive: 文件.rar\n 12% 文件.txt";
    for byte in text.as_bytes() {
        parser.push(&[*byte], &progress);
        assert!(!progress.output().unwrap().contains('\u{fffd}'));
    }
    assert_eq!(progress.output().unwrap(), text);
    parser.push(&erase(" 12% 文件.txt"), &progress);
    parser.push(" 51% 文件.txt".as_bytes(), &progress);
    assert!(progress.output().unwrap().ends_with(" 51% 文件.txt"));
    parser.push(b"\rEverything is Ok\n", &progress);
    let log = progress.output().unwrap();
    assert!(log.contains("Everything is Ok"));
    assert!(!log.contains(['\x08', '\r']));
    parser.push("newest 文件\n".repeat(10_000).as_bytes(), &progress);
    let log = progress.output().unwrap();
    assert!(log.len() <= super::OUTPUT_LIMIT);
    assert!(log.ends_with("newest 文件\n"));
    // stderr pipe chunks must not insert newlines or erase the live stdout line.
    parser.push(b"current file", &progress);
    progress.output_stderr("diagnos");
    progress.output_stderr("tic ");
    progress.output_stderr("文件");
    assert!(
        progress
            .output()
            .unwrap()
            .ends_with("current file\ndiagnostic 文件")
    );
    progress.output_stderr("\n");
    let log = progress.output().unwrap();
    assert!(log.contains("diagnostic 文件\n"));
    assert!(log.ends_with("current file"));
}

#[test]
fn rar_password_diagnostics_do_not_disable_empty_output_cleanup() {
    for text in [
        "ERROR: Wrong password : file.txt\n",
        "ERROR: Data Error in encrypted file. Wrong password? : file.txt\n",
        "ERROR: CRC Failed in encrypted file. Wrong password? : file.txt\n",
    ] {
        let progress = ProgressHandle::with_output();
        let mut parser = SevenZipOutput::default();
        parser.push(text.as_bytes(), &progress);
        assert!(parser.diagnostics.wrong_password);
        assert!(!parser.diagnostics.preserve_output);
        assert!(progress.output().unwrap().contains(text));
    }
}

#[cfg(unix)]
#[test]
fn command_output_streams_before_exit_and_remains_available_after_cancellation() {
    use crate::model::operations::OperationError;
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    let progress = ProgressHandle::with_output();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'Extracting archive: demo.rar\\n'; printf 'current file'; printf 'diagnostic\\n' >&2; while :; do :; done", "fixture"]);
        super::run_archive_command(command, &worker_progress)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let log = progress.output().unwrap();
        if log.contains("current file") && log.contains("diagnostic") {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let running = progress.output().unwrap();
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert!(
        running.contains("Extracting archive: demo.rar"),
        "{running}"
    );
    assert!(running.contains("current file"), "{running}");
    assert!(running.contains("diagnostic"), "{running}");
    assert!(progress.output().unwrap().contains("current file"));
}

fn erase(text: &str) -> Vec<u8> {
    [
        vec![8; text.len()],
        vec![b' '; text.len()],
        vec![8; text.len()],
    ]
    .concat()
}

#[test]
fn progress_phases_share_cancellation_and_map_nested_percentages() {
    let progress = ProgressHandle::default();
    let phase = progress.phase(30., 70.);
    phase.update(50., "middle phase");
    assert_eq!(progress.snapshot().percent, 50.);
    let nested = phase.phase(50., 100.);
    nested.update(50., "nested phase");
    assert_eq!(progress.snapshot().percent, 60.);
    phase.update(100., "phase complete");
    assert_eq!(progress.snapshot().percent, 70.);
    let mut output = SevenZipOutput::default();
    progress.update(50., "same item");
    output.push(b" 50% same item", &progress.phase(35., 100.));
    assert_eq!(progress.snapshot().percent, 67.5);
    progress.cancel();
    assert!(phase.is_cancelled() && nested.is_cancelled());
}

#[cfg(unix)]
#[test]
fn binary_archive_stream_separates_progress_and_preserves_partial_output_on_cancel() {
    use super::run_archive_command_to_file;
    use crate::model::operations::OperationError;
    use std::{
        fs,
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("partial.tar");
    let file = fs::File::create(&path).unwrap();
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf 'binary archive data'; printf ' 42%% + outer stream' >&2; while :; do sleep 0.01; done", "binary-fixture"]);
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker =
        thread::spawn(move || run_archive_command_to_file(command, file, &worker_progress));
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress.snapshot().percent != 42. && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let observed = progress.snapshot();
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(observed.percent, 42.);
    assert_eq!(fs::read(path).unwrap(), b"binary archive data");
}

#[test]
fn inline_progress_is_published_before_newline_or_eof() {
    let progress = ProgressHandle::default();
    let mut output = SevenZipOutput::default();
    output.push(b"  0%", &progress);
    output.push(&erase("  0%"), &progress);
    output.push(b" 4", &progress);
    output.push(b"5", &progress);
    assert_eq!(progress.snapshot().percent, 0.);
    output.push(b"%", &progress);
    assert_eq!(progress.snapshot().percent, 45.);
    output.push(b" + payload.bin", &progress);
    assert_eq!(progress.snapshot().item, "+ payload.bin");
    output.push(&erase(" 45% + payload.bin"), &progress);
    output.push(b" 90% 1 + payload.bin", &progress);
    assert_eq!(progress.snapshot().percent, 90.);
    assert_eq!(progress.snapshot().item, "1 + payload.bin");
    // Finishing an output stream alone must not fabricate completion.
    output.finish_line(&progress);
    assert_eq!(progress.snapshot().percent, 90.);
    assert!(output.detail.is_empty());
}

#[test]
fn progress_survives_every_pipe_chunk_boundary_and_inline_erasure() {
    let first = " 23% + 文件.bin";
    let second = " 67% 1 + 文件.bin";
    let bytes = [
        first.as_bytes().to_vec(),
        erase(first),
        second.as_bytes().to_vec(),
        erase(second),
    ]
    .concat();
    for boundary in 0..=bytes.len() {
        let progress = ProgressHandle::default();
        let mut output = SevenZipOutput::default();
        output.push(&bytes[..boundary], &progress);
        output.push(&bytes[boundary..], &progress);
        assert_eq!(progress.snapshot().percent, 67., "boundary {boundary}");
        assert_eq!(
            progress.snapshot().item,
            "1 + 文件.bin",
            "boundary {boundary}"
        );
        assert!(!output.diagnostics.preserve_output);
    }
    let progress = ProgressHandle::default();
    let mut output = SevenZipOutput::default();
    for byte in bytes {
        output.push(&[byte], &progress);
    }
    assert_eq!(progress.snapshot().percent, 67.);
    assert_eq!(progress.snapshot().item, "1 + 文件.bin");
}

#[test]
fn progress_and_diagnostics_remain_separate_in_verbose_output() {
    let progress = ProgressHandle::default();
    let mut output = SevenZipOutput::default();
    output.push(
        b"Creating archive: split.7z\n 12% + first.bin\r 51% + second.bin\n",
        &progress,
    );
    assert_eq!(progress.snapshot().percent, 51.);
    output.push(b" 78% + last.bin", &progress);
    output.push(&erase(" 78% + last.bin"), &progress);
    output.push(
        b"ERROR: CRC Failed : trailing space.bin \nERROR: Wrong password : secret.bin",
        &progress,
    );
    output.finish_line(&progress);
    assert_eq!(progress.snapshot().percent, 78.);
    assert!(
        output
            .diagnostics
            .damaged_entries
            .contains("trailing space.bin ")
    );
    assert!(output.diagnostics.wrong_password);
    assert!(!output.diagnostics.preserve_output);
    assert!(output.detail.contains("Creating archive:"));
    assert!(output.detail.contains("ERROR: CRC Failed"));
    assert!(!output.detail.contains("51%"));
    for text in ["CRC Failed : 10% file\n", "NaN%\n", "101%\n", "-1%\n"] {
        output.push(text.as_bytes(), &progress);
        assert_eq!(progress.snapshot().percent, 78.);
    }
}

#[cfg(unix)]
#[test]
fn child_progress_is_visible_while_running_and_cancellation_keeps_partial_volume() {
    use super::run_7z_progress;
    use crate::model::operations::OperationError;
    use std::{
        fs,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    let root = tempfile::tempdir().unwrap();
    let volume = root.path().join("archive.7z.001");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf partial > \"$1\"; printf ' 42%% + payload.bin'; while :; do sleep 0.01; done",
            "progress-fixture",
        ])
        .arg(&volume);
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        run_7z_progress(command, &worker_progress).map_err(|error| error.error)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress.snapshot().percent != 42. && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let observed = progress.snapshot();
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(observed.percent, 42.);
    assert_eq!(fs::read(volume).unwrap(), b"partial");
    assert_eq!(progress.snapshot().percent, 42.);
}
