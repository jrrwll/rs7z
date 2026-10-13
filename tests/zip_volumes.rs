use rs7z::{
    model::archive::{
        ArchiveDestination, ArchiveFormat, ArchiveOperation, ArchiveOptions, CompressionLevel,
    },
    service::{
        archive,
        operations::{self, ProgressHandle},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn volumes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("bundle.zip.")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

#[test]
fn zip_volumes_roundtrip_plain_and_aes_and_remain_readonly() {
    for password in [None, Some("secret")] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("nested");
        fs::create_dir(&source).unwrap();
        let contents: Vec<u8> = (0..16_384).map(|ix| (ix % 251) as u8).collect();
        fs::write(source.join("payload.bin"), &contents).unwrap();
        let path = root.path().join("bundle.zip");
        let first = root.path().join("bundle.zip.001");
        let mut options = ArchiveOptions::default();
        options.volume_size = Some(1024);
        options.compression_level = Some(CompressionLevel::Store);
        options.password = password.map(str::to_owned);
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default(),
        )
        .unwrap();
        assert!(!path.exists());
        assert_eq!(fs::metadata(&first).unwrap().len(), 1024);
        assert!(root.path().join("bundle.zip.002").exists());
        let original = volumes(root.path());
        assert_eq!(archive::format(&first), Some(ArchiveFormat::Zip));
        assert_eq!(
            archive::format(Path::new("BUNDLE.ZIP.001")),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(archive::format(Path::new("bundle.zip.002")), None);
        assert_eq!(
            archive::extraction_directory(&first),
            root.path().join("bundle")
        );
        assert_eq!(archive::with_format(&first, ArchiveFormat::Zip), path);
        assert!(archive::is_split(&first));
        let index = archive::read(&first).unwrap();
        assert!(index.read_only);
        assert_eq!(index.encrypted, password.is_some());
        assert_eq!(
            archive::children(&index, "nested").unwrap()[0].size,
            contents.len() as u64
        );
        assert_eq!(
            archive::archive_path(&first.join("nested")),
            Some(first.clone())
        );
        options.volume_size = None;
        if let Some(password) = password {
            assert!(archive::verify_password(&first, "wrong", &ProgressHandle::default()).is_err());
            archive::verify_password(&first, password, &ProgressHandle::default()).unwrap();
            assert!(operations::test_archive(&first).is_err());
            let missing = root.path().join("no-password");
            assert!(operations::extract(&first, &missing).is_err());
            assert!(!missing.exists());
        }
        operations::test_archive_with_options(&first, &options, &ProgressHandle::default())
            .unwrap();
        let output = root.path().join("extracted");
        operations::extract_with_options(&first, &output, &options, &ProgressHandle::default())
            .unwrap();
        assert_eq!(
            fs::read(output.join("nested/payload.bin")).unwrap(),
            contents
        );
        let export = root.path().join("export");
        fs::create_dir(&export).unwrap();
        archive::apply_with_options(
            &first,
            &ArchiveOperation::Transfer {
                entries: vec!["nested/payload.bin".into()],
                destination: ArchiveDestination::FileSystem(export.clone()),
                move_entries: false,
            },
            &options,
            &ProgressHandle::default(),
        )
        .unwrap();
        assert_eq!(fs::read(export.join("payload.bin")).unwrap(), contents);
        for operation in [
            ArchiveOperation::Add {
                sources: vec![source.clone()],
                directory: "/".into(),
            },
            ArchiveOperation::Delete {
                entries: vec!["nested".into()],
            },
            ArchiveOperation::Rename {
                entry: "nested".into(),
                name: "renamed".into(),
            },
            ArchiveOperation::Transfer {
                entries: vec!["nested".into()],
                destination: ArchiveDestination::Directory("copy".into()),
                move_entries: false,
            },
            ArchiveOperation::Transfer {
                entries: vec!["nested".into()],
                destination: ArchiveDestination::FileSystem(export.clone()),
                move_entries: true,
            },
        ] {
            assert!(
                archive::apply_with_options(
                    &first,
                    &operation,
                    &options,
                    &ProgressHandle::default()
                )
                .is_err()
            );
        }
        // Both split and ordinary creation must reject an existing volume family.
        for volume_size in [None, Some(1024)] {
            options.volume_size = volume_size;
            assert!(
                operations::add_many_with_options(
                    std::slice::from_ref(&source),
                    &path,
                    &options,
                    &ProgressHandle::default()
                )
                .is_err()
            );
            assert!(
                operations::add_many_with_options(
                    std::slice::from_ref(&source),
                    &first,
                    &options,
                    &ProgressHandle::default()
                )
                .is_err()
            );
        }
        assert_eq!(volumes(root.path()), original);
        // A missing middle volume must not be accepted as a complete ZIP.
        fs::rename(
            root.path().join("bundle.zip.002"),
            root.path().join("missing-volume"),
        )
        .unwrap();
        options.volume_size = None;
        assert!(
            operations::test_archive_with_options(&first, &options, &ProgressHandle::default())
                .is_err()
        );
        assert!(
            operations::extract_with_options(
                &first,
                &root.path().join("incomplete"),
                &options,
                &ProgressHandle::default()
            )
            .is_err()
        );
    }
}

#[test]
fn zip_volume_preflight_preserves_existing_files_and_rejects_invalid_requests() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"contents").unwrap();
    let mut options = ArchiveOptions::default();
    options.volume_size = Some(1024);
    for name in [
        "existing.zip",
        "traditional.zip",
        "orphan.zip",
        "alias.jar",
        "alias.zipx",
    ] {
        let path = root.path().join(name);
        let protected = match name {
            "existing.zip" => Some(path.clone()),
            "traditional.zip" => Some(root.path().join("traditional.z01")),
            "orphan.zip" => Some(root.path().join("orphan.zip.003")),
            _ => None,
        };
        if let Some(path) = &protected {
            fs::write(path, b"do not replace").unwrap();
        }
        assert!(
            operations::add_many_with_options(
                std::slice::from_ref(&source),
                &path,
                &options,
                &ProgressHandle::default()
            )
            .is_err(),
            "accepted {name}"
        );
        if let Some(path) = &protected {
            assert_eq!(fs::read(path).unwrap(), b"do not replace");
        }
        assert!(!root.path().join(format!("{name}.001")).exists());
    }
    options.volume_size = Some(0);
    assert!(
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &root.path().join("zero.zip"),
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    options.volume_size = Some(1024);
    let progress = ProgressHandle::default();
    progress.cancel();
    assert!(
        operations::add_many_with_options(
            &[source],
            &root.path().join("cancel.zip"),
            &options,
            &progress
        )
        .is_err()
    );
    assert!(!root.path().join("cancel.zip.001").exists());
}

#[test]
fn cancelling_zip_compression_keeps_partial_volumes() {
    use rs7z::model::operations::OperationError;
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("payload.bin");
    // A sparse source avoids allocating a large fixture while Store mode keeps
    // the real CLI busy writing volumes long enough to cancel after it starts.
    fs::File::create(&source)
        .unwrap()
        .set_len(256 * 1024 * 1024)
        .unwrap();
    let path = root.path().join("bundle.zip");
    let first = root.path().join("bundle.zip.001");
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        let mut options = ArchiveOptions::default();
        options.volume_size = Some(1024 * 1024);
        options.compression_level = Some(CompressionLevel::Store);
        operations::add_many_with_options(&[source], &path, &options, &worker_progress)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !first.is_file() && !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert!(
        first.is_file(),
        "Cancellation must retain existing partial volumes"
    );
}
