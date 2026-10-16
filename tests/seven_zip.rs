use rs7z::{
    model::{
        archive::{
            ArchiveDestination, ArchiveFormat, ArchiveOperation, ArchiveOptions, CompressionLevel,
            SevenZipMethod, SolidBlockSize,
        },
        operations::OperationError,
    },
    service::{
        archive, file,
        operations::{self, ProgressHandle},
        zip::parse_volume_size,
    },
};
use std::{fs, path::Path};

fn create(source: &Path, path: &Path, options: &ArchiveOptions) {
    operations::add_many_with_options(
        &[source.to_owned()],
        path,
        options,
        &ProgressHandle::default(),
    )
    .unwrap();
}

fn apply(path: &Path, operation: ArchiveOperation, options: &ArchiveOptions) {
    archive::apply_with_options(path, &operation, options, &ProgressHandle::default()).unwrap();
}

fn has(path: &Path, name: &str, options: &ArchiveOptions) -> bool {
    archive::read_with_options(path, options)
        .unwrap()
        .entries
        .iter()
        .any(|entry| entry.name == name)
}

#[test]
fn volume_sizes_are_optional_positive_and_cannot_inject_options() {
    for (input, expected) in [
        ("", None),
        ("  ", None),
        ("123", Some(123)),
        ("10B", Some(10)),
        ("2k", Some(2048)),
        ("100 M", Some(104857600)),
        ("1G", Some(1073741824)),
    ] {
        assert_eq!(parse_volume_size(input).unwrap(), expected, "{input}");
    }
    for input in [
        "0",
        "0M",
        "-1M",
        "1.5G",
        "100M -psecret",
        "1T",
        "18446744073709551615G",
        "999999999999999999999999",
    ] {
        assert!(parse_volume_size(input).is_err(), "accepted {input}");
    }
}

#[test]
fn seven_zip_methods_and_sizes_create_extractable_archives() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"seven zip settings").unwrap();
    for (name, method, listing_name, word) in [
        ("lzma2", SevenZipMethod::Lzma2, "LZMA2", Some(32)),
        ("lzma", SevenZipMethod::Lzma, "LZMA", Some(32)),
        ("ppmd", SevenZipMethod::Ppmd, "PPMD", Some(16)),
        ("bzip2", SevenZipMethod::Bzip2, "BZip2", None),
    ] {
        let path = root.path().join(format!("{name}.7z"));
        let mut options = ArchiveOptions::default();
        options.seven_zip.method = Some(method);
        if method != SevenZipMethod::Bzip2 {
            options.seven_zip.dictionary_size = Some(16 * 1024 * 1024);
            options.seven_zip.word_size = word;
        }
        options.seven_zip.solid_block_size = Some(if method == SevenZipMethod::Bzip2 {
            SolidBlockSize::Size(4 * 1024 * 1024 * 1024)
        } else {
            SolidBlockSize::NonSolid
        });
        create(&source, &path, &options);
        let listing = rs7z::service::seven_zip_cli::command()
            .unwrap()
            .args(["l", "-slt", "--"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(
            listing.contains(&format!("Method = {listing_name}")),
            "{listing}"
        );
        operations::test_archive_with_progress(&path, &ProgressHandle::default()).unwrap();
        let output = root.path().join(format!("out-{name}"));
        operations::extract_with_progress(&path, &output, &ProgressHandle::default()).unwrap();
        assert_eq!(
            fs::read(output.join("item.txt")).unwrap(),
            b"seven zip settings"
        );
    }
}

#[test]
fn seven_zip_rejects_incompatible_method_options() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"contents").unwrap();
    let path = root.path().join("archive.7z");
    let mut options = ArchiveOptions::default();
    options.seven_zip.method = Some(SevenZipMethod::Bzip2);
    options.seven_zip.dictionary_size = Some(16 * 1024 * 1024);
    assert!(
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    options.seven_zip.method = Some(SevenZipMethod::Ppmd);
    options.seven_zip.word_size = Some(64);
    assert!(
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn seven_zip_solid_options_control_block_layout() {
    let root = tempfile::tempdir().unwrap();
    let sources = [
        root.path().join("first.txt"),
        root.path().join("second.txt"),
    ];
    fs::write(&sources[0], b"first file").unwrap();
    fs::write(&sources[1], b"second file").unwrap();
    for (name, block, expected) in [
        ("non-solid", SolidBlockSize::NonSolid, "Solid = -"),
        ("solid", SolidBlockSize::Solid, "Solid = +"),
        (
            "one-megabyte",
            SolidBlockSize::Size(1024 * 1024),
            "Solid = +",
        ),
    ] {
        let path = root.path().join(format!("{name}.7z"));
        let mut options = ArchiveOptions::default();
        options.seven_zip.solid_block_size = Some(block);
        operations::add_many_with_options(&sources, &path, &options, &ProgressHandle::default())
            .unwrap();
        let listing = rs7z::service::seven_zip_cli::command()
            .unwrap()
            .args(["l", "-slt", "--"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(listing.contains(expected), "{listing}");
    }
}

#[test]
fn seven_zip_tree_test_and_extraction_preserve_directories_contents_and_modified() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("docs");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::write(source.join("文件 [1].txt"), b"contents").unwrap();
    let path = root.path().join("archive.7z");
    let options = ArchiveOptions::default();
    create(&source, &path, &options);
    let index = archive::read(&path).unwrap();
    assert_eq!(index.format, ArchiveFormat::SevenZip);
    assert!(!index.encrypted);
    assert!(index.entries.iter().all(|entry| entry.modified.is_some()));
    assert_eq!(archive::children(&index, "docs").unwrap().len(), 2);
    assert_eq!(
        file::read_item(&path).unwrap().kind,
        rs7z::model::file::FileKind::Archive
    );
    assert_eq!(
        archive::archive_path(&path.join("docs")),
        Some(path.clone())
    );
    operations::test_archive_with_options(&path, &options, &ProgressHandle::default()).unwrap();
    let output = root.path().join("output");
    operations::extract_with_options(&path, &output, &options, &ProgressHandle::default()).unwrap();
    assert_eq!(
        fs::read(output.join("docs/文件 [1].txt")).unwrap(),
        b"contents"
    );
    assert!(output.join("docs/empty").is_dir());
}

#[test]
fn encrypted_headers_require_password_and_support_changes_without_losing_encryption() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("secret.txt");
    fs::write(&source, b"private").unwrap();
    let path = root.path().join("encrypted.7z");
    let mut options = ArchiveOptions::default();
    options.password = Some("secret".into());
    options.compression_level = Some(CompressionLevel::Maximum);
    create(&source, &path, &options);
    let locked = archive::read(&path).unwrap();
    assert!(locked.encrypted);
    assert!(locked.entries.is_empty());
    assert!(format!("{options:?}").contains("encrypted: true"));
    assert!(!format!("{options:?}").contains("secret"));
    assert!(
        matches!(operations::test_archive_with_progress(&path, &ProgressHandle::default()), Err(OperationError::Failed(error)) if error.contains("Password required"))
    );
    assert!(archive::verify_password(&path, "wrong", &ProgressHandle::default()).is_err());
    archive::verify_password(&path, "secret", &ProgressHandle::default()).unwrap();
    let original = fs::read(&path).unwrap();
    let mut wrong = options.clone();
    wrong.password = Some("wrong".into());
    let output = root.path().join("failed-output");
    assert!(
        operations::extract_with_options(&path, &output, &wrong, &ProgressHandle::default())
            .is_err()
    );
    assert!(!output.exists());
    assert_eq!(fs::read(&path).unwrap(), original);
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: "secret.txt".into(),
            name: "renamed.txt".into(),
        },
        &options,
    );
    assert!(has(&path, "renamed.txt", &options));
    assert!(archive::read(&path).unwrap().encrypted);
    operations::test_archive_with_options(&path, &options, &ProgressHandle::default()).unwrap();
}

#[test]
fn seven_zip_supports_add_multi_copy_move_rename_delete_and_export() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("docs");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    fs::write(source.join("b.txt"), b"b").unwrap();
    let path = root.path().join("edit.7z");
    let options = ArchiveOptions::default();
    create(&source, &path, &options);
    let extra = root.path().join("extra.txt");
    fs::write(&extra, b"extra").unwrap();
    apply(
        &path,
        ArchiveOperation::Add {
            sources: vec![extra],
            directory: "/new folder/sub".into(),
        },
        &options,
    );
    assert!(has(&path, "new folder/sub/extra.txt", &options));
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs/a.txt".into(), "docs/empty".into()],
            destination: ArchiveDestination::Directory("/copy".into()),
            move_entries: false,
        },
        &options,
    );
    assert!(has(&path, "copy/a.txt", &options));
    assert!(has(&path, "copy/empty", &options));
    assert!(has(&path, "docs/a.txt", &options));
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs/b.txt".into(), "new folder/sub/extra.txt".into()],
            destination: ArchiveDestination::Directory("/".into()),
            move_entries: true,
        },
        &options,
    );
    assert!(has(&path, "b.txt", &options));
    assert!(!has(&path, "docs/b.txt", &options));
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: "copy".into(),
            name: "renamed".into(),
        },
        &options,
    );
    let output = root.path().join("export");
    fs::create_dir(&output).unwrap();
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["renamed".into(), "b.txt".into()],
            destination: ArchiveDestination::FileSystem(output.clone()),
            move_entries: false,
        },
        &options,
    );
    assert_eq!(fs::read(output.join("renamed/a.txt")).unwrap(), b"a");
    assert!(output.join("renamed/empty").is_dir());
    assert_eq!(fs::read(output.join("b.txt")).unwrap(), b"b");
    let move_output = root.path().join("move");
    fs::create_dir(&move_output).unwrap();
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["b.txt".into(), "extra.txt".into()],
            destination: ArchiveDestination::FileSystem(move_output.clone()),
            move_entries: true,
        },
        &options,
    );
    assert!(!has(&path, "b.txt", &options));
    assert_eq!(fs::read(move_output.join("extra.txt")).unwrap(), b"extra");
    let entries = archive::read(&path)
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    apply(&path, ArchiveOperation::Delete { entries }, &options);
    assert!(archive::read(&path).unwrap().entries.is_empty());
    operations::test_archive_with_progress(&path, &ProgressHandle::default()).unwrap();
}

#[test]
fn collisions_invalid_paths_and_cancellation_leave_original_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("docs");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("item.txt"), b"contents").unwrap();
    let path = root.path().join("archive.7z");
    let options = ArchiveOptions::default();
    create(&source, &path, &options);
    let original = fs::read(&path).unwrap();
    for operation in [
        ArchiveOperation::Transfer {
            entries: vec!["docs/item.txt".into()],
            destination: ArchiveDestination::Directory("/docs".into()),
            move_entries: false,
        },
        ArchiveOperation::Transfer {
            entries: vec!["docs".into()],
            destination: ArchiveDestination::Directory("/docs/child".into()),
            move_entries: true,
        },
        ArchiveOperation::Rename {
            entry: "docs/item.txt".into(),
            name: "../escape".into(),
        },
        ArchiveOperation::Add {
            sources: vec![source.clone()],
            directory: "/".into(),
        },
    ] {
        assert!(archive::apply(&path, &operation, &ProgressHandle::default()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        archive::apply(
            &path,
            &ArchiveOperation::Delete {
                entries: vec!["docs".into()]
            },
            &cancelled
        ),
        Err(OperationError::Cancelled)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn split_archives_can_be_browsed_tested_extracted_and_exported_but_not_modified() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("large.bin");
    fs::write(&source, vec![42u8; 8192]).unwrap();
    let path = root.path().join("split.7z");
    let mut options = ArchiveOptions::default();
    options.volume_size = Some(1024);
    options.compression_level = Some(CompressionLevel::Store);
    create(&source, &path, &options);
    assert!(!path.exists());
    let first = root.path().join("split.7z.001");
    assert!(root.path().join("split.7z.002").exists());
    assert_eq!(fs::metadata(&first).unwrap().len(), 1024);
    assert!(has(&first, "large.bin", &ArchiveOptions::default()));
    assert_eq!(archive::format(&first), Some(ArchiveFormat::SevenZip));
    operations::test_archive_with_progress(&first, &ProgressHandle::default()).unwrap();
    let output = root.path().join("extract");
    operations::extract_with_progress(&first, &output, &ProgressHandle::default()).unwrap();
    assert_eq!(
        fs::read(output.join("large.bin")).unwrap(),
        vec![42u8; 8192]
    );
    let export = root.path().join("export");
    fs::create_dir(&export).unwrap();
    apply(
        &first,
        ArchiveOperation::Transfer {
            entries: vec!["large.bin".into()],
            destination: ArchiveDestination::FileSystem(export.clone()),
            move_entries: false,
        },
        &ArchiveOptions::default(),
    );
    assert_eq!(
        fs::read(export.join("large.bin")).unwrap(),
        vec![42u8; 8192]
    );
    let original = fs::read(&first).unwrap();
    for operation in [
        ArchiveOperation::Delete {
            entries: vec!["large.bin".into()],
        },
        ArchiveOperation::Rename {
            entry: "large.bin".into(),
            name: "new.bin".into(),
        },
        ArchiveOperation::Transfer {
            entries: vec!["large.bin".into()],
            destination: ArchiveDestination::Directory("new".into()),
            move_entries: false,
        },
        ArchiveOperation::Transfer {
            entries: vec!["large.bin".into()],
            destination: ArchiveDestination::FileSystem(export),
            move_entries: true,
        },
    ] {
        assert!(archive::apply(&first, &operation, &ProgressHandle::default()).is_err());
    }
    assert_eq!(fs::read(&first).unwrap(), original);
    assert!(
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert_eq!(fs::read(&first).unwrap(), original);
    assert!(
        operations::add_many_with_options(
            &[source],
            &root.path().join("bad.tar.gz"),
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
}

#[test]
fn split_compression_reports_intermediate_progress_before_completion() {
    use std::{
        io::Write,
        thread,
        time::{Duration, Instant},
    };

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("payload.bin");
    let mut file = fs::File::create(&source).unwrap();
    // Incompressible data keeps real 7z running across its progress update interval.
    let mut random = 0x9e3779b97f4a7c15_u64;
    let mut buffer = [0u8; 64 * 1024];
    for _ in 0..512 {
        for word in buffer.chunks_exact_mut(8) {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            word.copy_from_slice(&random.to_le_bytes());
        }
        file.write_all(&buffer).unwrap();
    }
    drop(file);
    let path = root.path().join("progress.7z");
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        let mut options = ArchiveOptions::default();
        options.volume_size = Some(1024 * 1024);
        options.compression_level = Some(CompressionLevel::Normal);
        operations::add_many_with_options(&[source], &path, &options, &worker_progress)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut observed_intermediate = false;
    while !worker.is_finished() && Instant::now() < deadline {
        let percent = progress.snapshot().percent;
        observed_intermediate |= percent > 0. && percent < 100.;
        thread::sleep(Duration::from_millis(5));
    }
    if !worker.is_finished() {
        progress.cancel();
    }
    worker.join().unwrap().unwrap();
    assert!(
        observed_intermediate,
        "No intermediate progress from split compression"
    );
    assert_eq!(progress.snapshot().percent, 100.);
    assert!(root.path().join("progress.7z.002").is_file());
}

#[test]
fn seven_zip_rejects_symbolic_links_before_extracting() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("links.7z");
    // libarchive fixture: p7zip 17 silently dereferences links even with -snl.
    fs::write(&path, include_bytes!("fixtures/symlink.7z")).unwrap();
    assert!(archive::read(&path).is_err());
    let destination = root.path().join("output");
    assert!(
        operations::extract_with_progress(&path, &destination, &ProgressHandle::default()).is_err()
    );
    assert!(!destination.exists());
}
