use rs7z::model::{
    operations::OperationError,
    zip::{CompressionLevel, ZipOptions},
};
use rs7z::service::operations::{
    ProgressHandle, add, add_many, add_many_with_progress, copy_or_move, copy_or_move_many,
    copy_or_move_with_progress, delete_many, delete_many_with_progress, extract,
    extract_with_progress, rename_with_progress, test_archive, test_archive_with_progress,
};
use rs7z::service::operations::{
    add_many_with_options, extract_with_options, test_archive_with_options,
};
use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

#[test]
fn rename_files_and_directories_preserves_contents_and_rejects_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("source.txt");
    fs::write(&file, b"contents").unwrap();
    let folder = root.path().join("folder");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("child.txt"), b"child").unwrap();
    for name in [
        "",
        " ",
        ".",
        "..",
        "...",
        "../escape",
        "new/child",
        "bad\\name",
        "bad\0name",
        "source.txt",
        "folder",
    ] {
        assert!(
            rename_with_progress(&file, name, &ProgressHandle::default()).is_err(),
            "accepted {name:?}"
        );
        assert_eq!(fs::read(&file).unwrap(), b"contents");
        assert_eq!(fs::read(folder.join("child.txt")).unwrap(), b"child");
    }
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        rename_with_progress(&file, "renamed.txt", &cancelled),
        Err(OperationError::Cancelled)
    );
    assert!(file.exists());
    assert!(!root.path().join("renamed.txt").exists());
    let progress = ProgressHandle::default();
    rename_with_progress(&file, "SOURCE.txt", &progress).unwrap();
    let file = root.path().join("SOURCE.txt");
    assert!(
        fs::read_dir(root.path())
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "SOURCE.txt")
    );
    rename_with_progress(&file, "新名称 [1].txt", &progress).unwrap();
    assert!(!file.exists());
    assert_eq!(
        fs::read(root.path().join("新名称 [1].txt")).unwrap(),
        b"contents"
    );
    assert_eq!(progress.snapshot().percent, 100.);
    rename_with_progress(&folder, "new folder", &ProgressHandle::default()).unwrap();
    assert!(!folder.exists());
    assert_eq!(
        fs::read(root.path().join("new folder/child.txt")).unwrap(),
        b"child"
    );
}

#[cfg(unix)]
#[test]
fn rename_rejects_dangling_link_at_destination_and_can_rename_a_link() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"source").unwrap();
    let link = root.path().join("link");
    symlink("missing", &link).unwrap();
    fs::hard_link(&source, root.path().join("hard-link")).unwrap();
    assert!(rename_with_progress(&source, "hard-link", &ProgressHandle::default()).is_err());
    assert!(rename_with_progress(&source, "link", &ProgressHandle::default()).is_err());
    rename_with_progress(&link, "renamed-link", &ProgressHandle::default()).unwrap();
    assert_eq!(
        fs::read_link(root.path().join("renamed-link")).unwrap(),
        PathBuf::from("missing")
    );
    assert_eq!(fs::read(source).unwrap(), b"source");
}

fn temp_dir() -> PathBuf {
    loop {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rs7z-{}-{nonce}-{sequence}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("Cannot create test directory {}: {error}", path.display()),
        }
    }
}

#[test]
fn copy_directory_rejects_child_destination() {
    let root = temp_dir();
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file.txt"), b"content").unwrap();
    assert!(copy_or_move(&source, &source.join("nested"), false).is_err());
    let target = root.join("target");
    copy_or_move(&source, &target, false).unwrap();
    assert_eq!(fs::read(target.join("file.txt")).unwrap(), b"content");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zip_round_trip() {
    let root = temp_dir();
    let first = root.join("first.txt");
    let second = root.join("second.txt");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();
    let archive = root.join("contents.zip");
    add(&first, &archive).unwrap();
    add(&second, &archive).unwrap();
    test_archive(&archive).unwrap();
    let destination = root.join("extracted");
    extract(&archive, &destination).unwrap();
    assert_eq!(fs::read(destination.join("first.txt")).unwrap(), b"first");
    assert_eq!(fs::read(destination.join("second.txt")).unwrap(), b"second");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_copy_move_and_delete_preflight_conflicts() {
    let root = temp_dir();
    let sources = [root.join("first.txt"), root.join("second.txt")];
    for source in &sources {
        fs::write(source, source.file_name().unwrap().as_encoded_bytes()).unwrap();
    }
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("second.txt"), b"existing").unwrap();
    assert!(copy_or_move_many(&sources, &target, false).is_err());
    assert!(!target.join("first.txt").exists());
    fs::remove_file(target.join("second.txt")).unwrap();
    copy_or_move_many(&sources, &target, false).unwrap();
    assert!(sources.iter().all(|source| source.exists()));

    let moved = root.join("moved");
    fs::create_dir(&moved).unwrap();
    copy_or_move_many(&sources, &moved, true).unwrap();
    assert!(sources.iter().all(|source| !source.exists()));
    let moved_sources = [moved.join("first.txt"), moved.join("second.txt")];
    delete_many(&moved_sources).unwrap();
    assert!(moved_sources.iter().all(|source| !source.exists()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_add_places_all_files_in_one_archive() {
    let root = temp_dir();
    let sources = [root.join("first.txt"), root.join("second.txt")];
    fs::write(&sources[0], b"first").unwrap();
    fs::write(&sources[1], b"second").unwrap();
    let archive = root.join("all.zip");
    add_many(&sources, &archive).unwrap();
    test_archive(&archive).unwrap();
    let output = root.join("output");
    extract(&archive, &output).unwrap();
    assert_eq!(fs::read(output.join("first.txt")).unwrap(), b"first");
    assert_eq!(fs::read(output.join("second.txt")).unwrap(), b"second");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zip_options_create_aes_archive_without_zipcrypto() {
    let root = temp_dir();
    let source = root.join("source.txt");
    fs::write(&source, b"secret contents").unwrap();
    let archive = root.join("encrypted.zip");
    let mut options = ZipOptions::default();
    options.password = Some("secret".into());
    options.compression_level = Some(CompressionLevel::Store);
    add_many_with_options(&[source], &archive, &options, &ProgressHandle::default()).unwrap();
    let error =
        test_archive_with_options(&archive, &ZipOptions::default(), &ProgressHandle::default())
            .unwrap_err();
    assert!(
        matches!(error, OperationError::Failed(message) if message.contains("Password required"))
    );
    test_archive_with_options(&archive, &options, &ProgressHandle::default()).unwrap();
    let output = root.join("output");
    extract_with_options(&archive, &output, &options, &ProgressHandle::default()).unwrap();
    assert_eq!(
        fs::read(output.join("source.txt")).unwrap(),
        b"secret contents"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_zip_probe_reads_metadata_without_extracting_payloads() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    fs::write(&source, b"encrypted payload").unwrap();
    for method in ["AES256", "ZipCrypto"] {
        let archive = root.path().join(format!("{method}.zip"));
        let output = rs7z::service::seven_zip_cli::command()
            .expect("official 7zz is required for archive tests")
            .args(["a", "-tzip", "-mx=0", "-psecret"])
            .arg(format!("-mem={method}"))
            .arg(&archive)
            .arg(&source)
            .output()
            .unwrap();
        assert!(output.status.success());

        let mut options = ZipOptions::default();
        options.password = Some("secret".into());
        test_archive_with_options(&archive, &options, &ProgressHandle::default()).unwrap();
        let output = root.path().join(format!("{method}-output"));
        extract_with_options(&archive, &output, &options, &ProgressHandle::default()).unwrap();
        assert_eq!(
            fs::read(output.join("source.txt")).unwrap(),
            b"encrypted payload"
        );

        // Damage encrypted data while leaving the directory metadata intact.
        // The missing-password result must not depend on reading that payload.
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
        let start = zip.by_index_raw(0).unwrap().data_start() as usize;
        drop(zip);
        let mut bytes = fs::read(&archive).unwrap();
        bytes[start] ^= 0xff;
        fs::write(&archive, &bytes).unwrap();
        let missing_password = ZipOptions::default();
        let progress = ProgressHandle::default();
        let error = test_archive_with_options(&archive, &missing_password, &progress).unwrap_err();
        assert!(
            matches!(error, OperationError::Failed(message) if message.contains("Password required"))
        );
        assert_eq!(progress.snapshot().percent, 0.);
        let destination = root.path().join(format!("{method}-blocked"));
        let error =
            extract_with_options(&archive, &destination, &missing_password, &progress).unwrap_err();
        assert!(
            matches!(error, OperationError::Failed(message) if message.contains("Password required"))
        );
        assert!(!destination.exists());
        let error = add_many_with_options(
            std::slice::from_ref(&source),
            &archive,
            &missing_password,
            &progress,
        )
        .unwrap_err();
        assert!(
            matches!(error, OperationError::Failed(message) if message.contains("Password required"))
        );
        assert_eq!(fs::read(&archive).unwrap(), bytes);
    }
}

#[test]
fn wrong_password_cleans_empty_extract_artifacts_and_preserves_existing_directories() {
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    let root = tempfile::tempdir().unwrap();
    for method in ["AES256", "ZipCrypto"] {
        let sources = root.path().join(method);
        fs::create_dir_all(sources.join("nested/deep")).unwrap();
        fs::write(sources.join("nested/deep/secret.txt"), b"secret contents").unwrap();
        let archive = root.path().join(format!("{method}.zip"));
        let output = rs7z::service::seven_zip_cli::command()
            .expect("official 7zz is required for archive tests")
            .current_dir(&sources)
            .args(["a", "-tzip", "-psecret"])
            .arg(format!("-mem={method}"))
            .arg(&archive)
            .arg("nested")
            .output()
            .unwrap();
        assert!(output.status.success());
        let archive_before = fs::read(&archive).unwrap();
        let mut options = ZipOptions::default();
        options.password = Some("incorrect".into());
        for existing in [false, true] {
            let parent = root.path().join(format!("{method}-{existing}"));
            let destination = parent.join("new/extracted");
            if existing {
                fs::create_dir_all(&destination).unwrap();
            }
            let error =
                extract_with_options(&archive, &destination, &options, &ProgressHandle::default())
                    .unwrap_err();
            assert!(
                matches!(error, OperationError::Failed(message) if message.contains("Wrong password"))
            );
            if existing {
                assert!(destination.is_dir());
                assert!(fs::read_dir(&destination).unwrap().next().is_none());
            } else {
                assert!(!parent.exists(), "left extraction artifacts for {method}");
            }
        }
        assert_eq!(fs::read(&archive).unwrap(), archive_before);
        options.password = Some("secret".into());
        let destination = root.path().join(format!("{method}-correct"));
        extract_with_options(&archive, &destination, &options, &ProgressHandle::default()).unwrap();
        assert_eq!(
            fs::read(destination.join("nested/deep/secret.txt")).unwrap(),
            b"secret contents"
        );
    }

    // Mixed ZIPs can successfully extract plaintext entries before a password failure.
    let archive = root.path().join("mixed.zip");
    let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
    zip.start_file("plain/keep.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"successfully extracted").unwrap();
    zip.start_file(
        "encrypted/remove.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
    )
    .unwrap();
    zip.write_all(b"secret").unwrap();
    zip.finish().unwrap();
    let mut options = ZipOptions::default();
    options.password = Some("incorrect".into());
    let destination = root.path().join("mixed-output");
    assert!(
        extract_with_options(&archive, &destination, &options, &ProgressHandle::default()).is_err()
    );
    assert_eq!(
        fs::read(destination.join("plain/keep.txt")).unwrap(),
        b"successfully extracted"
    );
    assert!(!destination.join("encrypted").exists());
}

#[test]
fn extract_removes_corrupt_files_and_preserves_successful_entries_and_cancellation() {
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};
    let root = tempfile::tempdir().unwrap();
    for compression in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let archive = root.path().join(format!("damaged-{compression:?}.zip"));
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.add_directory("empty/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("keep.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"successfully extracted").unwrap();
        zip.start_file("keep-empty.txt", SimpleFileOptions::default())
            .unwrap();
        zip.start_file(
            "nested/损坏 file.txt",
            SimpleFileOptions::default().compression_method(compression),
        )
        .unwrap();
        zip.write_all(b"original data").unwrap();
        zip.finish().unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
        let start = zip.by_name("nested/损坏 file.txt").unwrap().data_start() as usize;
        drop(zip);
        let mut bytes = fs::read(&archive).unwrap();
        bytes[start] ^= 0xff;
        fs::write(&archive, bytes).unwrap();
        let destination = root.path().join(format!("output-{compression:?}"));
        let error =
            extract_with_progress(&archive, &destination, &ProgressHandle::default()).unwrap_err();
        assert!(
            matches!(error, OperationError::Failed(message) if !message.contains("Wrong password"))
        );
        assert!(destination.join("empty").is_dir());
        assert!(!destination.join("nested/损坏 file.txt").exists());
        assert_eq!(
            fs::read(destination.join("keep.txt")).unwrap(),
            b"successfully extracted"
        );
        assert!(destination.join("keep-empty.txt").is_file());

        // The legacy extraction entry point must use the same cleanup policy.
        let legacy = root.path().join(format!("legacy-{compression:?}"));
        assert!(extract(&archive, &legacy).is_err());
        assert!(!legacy.join("nested/损坏 file.txt").exists());
        assert_eq!(
            fs::read(legacy.join("keep.txt")).unwrap(),
            b"successfully extracted"
        );

        let cancelled = ProgressHandle::default();
        cancelled.cancel();
        let new_destination = root.path().join("cancelled");
        assert_eq!(
            extract_with_progress(&archive, &new_destination, &cancelled),
            Err(OperationError::Cancelled)
        );
        assert!(!new_destination.exists());
        assert_eq!(
            extract_with_progress(&archive, &destination, &cancelled),
            Err(OperationError::Cancelled)
        );
        assert!(destination.join("empty").is_dir());
        assert_eq!(
            fs::read(destination.join("keep.txt")).unwrap(),
            b"successfully extracted"
        );
    }
}

#[test]
fn progress_copy_move_delete_and_cancel_without_cleanup() {
    let root = temp_dir();
    let source = root.join("source.txt");
    fs::write(&source, vec![42; 1024 * 1024]).unwrap();
    let target = root.join("target.txt");
    let progress = ProgressHandle::default();
    copy_or_move_with_progress(std::slice::from_ref(&source), &target, false, &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    assert_eq!(fs::metadata(&target).unwrap().len(), 1024 * 1024);

    let moved = root.join("moved.txt");
    let progress = ProgressHandle::default();
    copy_or_move_with_progress(std::slice::from_ref(&target), &moved, true, &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    assert!(!target.exists());

    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        delete_many_with_progress(std::slice::from_ref(&moved), &cancelled),
        Err(OperationError::Cancelled)
    );
    assert!(moved.exists());
    assert_eq!(
        copy_or_move_with_progress(std::slice::from_ref(&source), &target, false, &cancelled),
        Err(OperationError::Cancelled)
    );
    assert!(!target.exists());

    let progress = ProgressHandle::default();
    delete_many_with_progress(&[moved], &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn progress_copy_deep_directory_on_small_worker_stack() {
    let root = temp_dir();
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir(&source).unwrap();
    let mut nested = PathBuf::new();
    for level in 0..24 {
        nested.push(format!("level-{level}"));
        let directory = source.join(&nested);
        fs::create_dir(&directory).unwrap();
        for file in 0..8 {
            fs::write(directory.join(format!("file-{file}.txt")), b"small file").unwrap();
        }
    }
    let source_for_worker = source.clone();
    let target_for_worker = target.clone();
    let snapshot = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let progress = ProgressHandle::default();
            copy_or_move_with_progress(&[source_for_worker], &target_for_worker, false, &progress)
                .unwrap();
            progress.snapshot()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(snapshot.percent, 100.);
    assert_eq!(
        fs::read(target.join(nested).join("file-7.txt")).unwrap(),
        b"small file"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zip_progress_and_cancel() {
    let root = temp_dir();
    let source = root.join("source.txt");
    fs::write(&source, b"archive contents").unwrap();
    let archive = root.join("archive.zip");
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        add_many_with_progress(std::slice::from_ref(&source), &archive, &cancelled),
        Err(OperationError::Cancelled)
    );
    assert!(!archive.exists());

    let progress = ProgressHandle::default();
    add_many_with_progress(&[source], &archive, &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    let progress = ProgressHandle::default();
    test_archive_with_progress(&archive, &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    let destination = root.join("extracted");
    let progress = ProgressHandle::default();
    extract_with_progress(&archive, &destination, &progress).unwrap();
    assert_eq!(progress.snapshot().percent, 100.);
    assert_eq!(
        fs::read(destination.join("source.txt")).unwrap(),
        b"archive contents"
    );
    fs::remove_dir_all(root).unwrap();
}
