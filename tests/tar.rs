use rs7z::{
    model::{
        archive::{ArchiveDestination, ArchiveFormat, ArchiveOperation, ArchiveOptions, TarFormat},
        file::FileKind,
        operations::OperationError,
    },
    service::{
        archive, file,
        operations::{self, ProgressHandle},
    },
};
use std::{fs, path::Path};

fn create(path: &Path, source: &Path) {
    operations::add_many_with_progress(&[source.to_owned()], path, &ProgressHandle::default())
        .unwrap();
}

fn apply(path: &Path, operation: ArchiveOperation) {
    archive::apply(path, &operation, &ProgressHandle::default()).unwrap();
}

fn has(path: &Path, name: &str) -> bool {
    archive::read(path)
        .unwrap()
        .entries
        .iter()
        .any(|entry| entry.name == name)
}

#[test]
fn tar_extensions_aliases_and_default_names() {
    for (format, aliases) in [
        (ArchiveFormat::Tar, vec!["tar"]),
        (ArchiveFormat::TarGzip, vec!["tar.gz", "tgz"]),
        (ArchiveFormat::TarBzip2, vec!["tar.bz2", "tbz", "tbz2"]),
        (ArchiveFormat::TarXz, vec!["tar.xz", "txz"]),
        (ArchiveFormat::TarLzma, vec!["tar.lzma", "tlz"]),
        (ArchiveFormat::TarZstd, vec!["tar.zst", "tzst"]),
        (ArchiveFormat::TarCompress, vec!["tar.z", "taz"]),
    ] {
        for extension in aliases {
            for extension in [extension.to_owned(), extension.to_uppercase()] {
                let path = Path::new("/tmp").join(format!("some.backup.{extension}"));
                assert_eq!(archive::format(&path), Some(format));
                assert_eq!(
                    archive::extraction_directory(&path),
                    Path::new("/tmp/some.backup")
                );
                assert_eq!(
                    archive::is_read_only(&path),
                    format == ArchiveFormat::TarCompress
                );
                assert_eq!(archive::with_format(&path, format), path);
                assert_eq!(
                    archive::with_format(&path, ArchiveFormat::Zip),
                    Path::new("/tmp/some.backup.zip")
                );
            }
        }
    }
    for name in [
        "archive.tar.gz.txt",
        "archive.tar.lzma86",
        "archive.tar.b64",
    ] {
        assert!(archive::format(Path::new(name)).is_none());
    }
    assert_eq!(
        archive::with_format(Path::new("file.zip"), ArchiveFormat::TarGzip),
        Path::new("file.tar.gz")
    );
}

#[test]
fn tar_creation_defaults_to_gnu_and_can_use_posix_headers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"tar format contents").unwrap();
    for (name, format, magic) in [
        ("gnu.tar", TarFormat::Gnu, b"ustar ".as_slice()),
        ("posix.tar", TarFormat::Posix, b"ustar\0".as_slice()),
    ] {
        let path = root.path().join(name);
        let mut options = ArchiveOptions::default();
        options.tar_format = format;
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default(),
        )
        .unwrap();
        assert_eq!(&fs::read(&path).unwrap()[257..263], magic);
        assert!(has(&path, "item.txt"));
        operations::test_archive(&path).unwrap();
        let output = root.path().join(format!("out-{name}"));
        operations::extract(&path, &output).unwrap();
        assert_eq!(
            fs::read(output.join("item.txt")).unwrap(),
            b"tar format contents"
        );
    }
}

#[test]
fn posix_header_format_is_preserved_inside_compressed_tar() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"compressed POSIX tar").unwrap();
    let path = root.path().join("archive.tar.gz");
    let mut options = ArchiveOptions::default();
    options.tar_format = TarFormat::Posix;
    operations::add_many_with_options(
        std::slice::from_ref(&source),
        &path,
        &options,
        &ProgressHandle::default(),
    )
    .unwrap();
    let output = rs7z::service::seven_zip_cli::command()
        .unwrap()
        .args(["x", "-tgzip", "-so", "--"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(&output.stdout[257..263], b"ustar\0");
    assert!(has(&path, "item.txt"));
    operations::test_archive(&path).unwrap();
}

#[test]
fn tar_aliases_browse_and_extract_contents_instead_of_an_intermediate_tar() {
    let root = tempfile::tempdir().unwrap();
    let docs = root.path().join("docs");
    fs::create_dir_all(docs.join("nested/empty")).unwrap();
    fs::write(docs.join("nested/文件 [1].txt"), b"tar contents").unwrap();
    for extensions in [
        vec!["tar"],
        vec!["tar.gz", "tgz"],
        vec!["tar.bz2", "tbz", "tbz2"],
        vec!["tar.xz", "txz"],
        vec!["tar.lzma", "tlz"],
        vec!["tar.zst", "tzst"],
    ] {
        let original = root.path().join(format!("archive.{}", extensions[0]));
        create(&original, &docs);
        for extension in extensions {
            let path = root.path().join(format!("copy.{extension}"));
            fs::copy(&original, &path).unwrap();
            let index = archive::read(&path).unwrap();
            assert!(!index.encrypted);
            assert_eq!(archive::children(&index, "docs/nested").unwrap().len(), 2);
            assert!(index.entries.iter().all(|entry| entry.modified.is_some()));
            assert_eq!(file::read_item(&path).unwrap().kind, FileKind::Archive);
            assert_eq!(
                archive::archive_path(&path.join("docs/nested")),
                Some(path.clone())
            );
            operations::test_archive(&path).unwrap();
            let output = root.path().join(format!("out-{extension}"));
            operations::extract(&path, &output).unwrap();
            assert_eq!(
                fs::read(output.join("docs/nested/文件 [1].txt")).unwrap(),
                b"tar contents"
            );
            assert!(output.join("docs/nested/empty").is_dir());
            assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
            assert!(operations::extract(&path, &output).is_err());
        }
    }
}

#[test]
fn tar_supports_add_multi_copy_move_rename_delete_and_export() {
    for extension in ["tar", "tar.gz", "tar.bz2", "tar.xz", "tar.lzma", "tar.zst"] {
        let root = tempfile::tempdir().unwrap();
        let docs = root.path().join("docs");
        fs::create_dir_all(docs.join("empty")).unwrap();
        fs::write(docs.join("a.txt"), b"alpha").unwrap();
        fs::write(docs.join("b.txt"), b"beta").unwrap();
        let path = root.path().join(format!("archive.{extension}"));
        create(&path, &docs);
        let extra = root.path().join("extra.txt");
        fs::write(&extra, b"extra").unwrap();
        apply(
            &path,
            ArchiveOperation::Add {
                sources: vec![extra],
                directory: "/target/new folder".into(),
            },
        );
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["docs/a.txt".into(), "docs/empty".into()],
                destination: ArchiveDestination::Directory("/copies".into()),
                move_entries: false,
            },
        );
        assert!(has(&path, "copies/a.txt") && has(&path, "copies/empty"));
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["docs/a.txt".into(), "docs/b.txt".into()],
                destination: ArchiveDestination::Directory("/".into()),
                move_entries: true,
            },
        );
        assert!(has(&path, "a.txt") && !has(&path, "docs/a.txt"));
        apply(
            &path,
            ArchiveOperation::Rename {
                entry: "target".into(),
                name: "renamed".into(),
            },
        );
        assert!(has(&path, "renamed/new folder/extra.txt"));
        let output = root.path().join("output");
        fs::create_dir(&output).unwrap();
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["renamed".into(), "a.txt".into()],
                destination: ArchiveDestination::FileSystem(output.clone()),
                move_entries: false,
            },
        );
        assert_eq!(fs::read(output.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(
            fs::read(output.join("renamed/new folder/extra.txt")).unwrap(),
            b"extra"
        );
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["b.txt".into(), "copies".into()],
                destination: ArchiveDestination::FileSystem(output.clone()),
                move_entries: true,
            },
        );
        assert!(!has(&path, "copies") && !has(&path, "b.txt"));
        assert_eq!(fs::read(output.join("copies/a.txt")).unwrap(), b"alpha");
        let entries = archive::children(&archive::read(&path).unwrap(), "")
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        apply(&path, ArchiveOperation::Delete { entries });
        assert!(archive::read(&path).unwrap().entries.is_empty());
        operations::test_archive(&path).unwrap();
    }
}

#[test]
fn tar_rejects_unsupported_options_collisions_and_pre_cancelled_changes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("file.txt");
    fs::write(&source, b"contents").unwrap();
    let path = root.path().join("archive.tgz");
    let mut options = ArchiveOptions::default();
    options.password = Some("not-supported".into());
    assert!(
        operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    options.password = None;
    options.volume_size = Some(1024);
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
    create(&path, &source);
    let before = fs::read(&path).unwrap();
    assert!(
        archive::apply(
            &path,
            &ArchiveOperation::Add {
                sources: vec![source],
                directory: "/".into()
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert!(matches!(
        archive::apply(
            &path,
            &ArchiveOperation::Delete {
                entries: vec!["file.txt".into()]
            },
            &cancelled
        ),
        Err(OperationError::Cancelled)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(matches!(
        operations::extract_with_progress(&path, &root.path().join("cancelled"), &cancelled),
        Err(OperationError::Cancelled)
    ));
    assert!(!root.path().join("cancelled").exists());
}

#[test]
fn tar_compress_aliases_are_read_only_but_support_test_extract_and_export() {
    let root = tempfile::tempdir().unwrap();
    for (ix, suffix) in ["tar.Z", "tar.z", "taz"].into_iter().enumerate() {
        let path = root.path().join(format!("legacy-{ix}.{suffix}"));
        fs::write(&path, include_bytes!("fixtures/sample.tar.Z")).unwrap();
        let original = fs::read(&path).unwrap();
        let index = archive::read(&path).unwrap();
        assert_eq!(index.format, ArchiveFormat::TarCompress);
        assert!(
            index
                .entries
                .iter()
                .any(|entry| entry.name == "docs/item.txt")
        );
        operations::test_archive(&path).unwrap();
        let output = root.path().join(format!("extracted-{ix}"));
        operations::extract(&path, &output).unwrap();
        assert_eq!(
            fs::read(output.join("docs/item.txt")).unwrap(),
            b"legacy tar contents\n"
        );
        let exported = root.path().join(format!("exported-{ix}"));
        fs::create_dir(&exported).unwrap();
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["docs/item.txt".into(), "docs/empty".into()],
                destination: ArchiveDestination::FileSystem(exported.clone()),
                move_entries: false,
            },
        );
        assert_eq!(
            fs::read(exported.join("item.txt")).unwrap(),
            b"legacy tar contents\n"
        );
        assert!(exported.join("empty").is_dir());
        for operation in [
            ArchiveOperation::Delete {
                entries: vec!["docs".into()],
            },
            ArchiveOperation::Rename {
                entry: "docs".into(),
                name: "changed".into(),
            },
            ArchiveOperation::Add {
                sources: vec![exported.join("item.txt")],
                directory: "/".into(),
            },
            ArchiveOperation::Transfer {
                entries: vec!["docs".into()],
                destination: ArchiveDestination::Directory("/new".into()),
                move_entries: false,
            },
            ArchiveOperation::Transfer {
                entries: vec!["docs".into()],
                destination: ArchiveDestination::FileSystem(exported.clone()),
                move_entries: true,
            },
        ] {
            assert!(archive::apply(&path, &operation, &ProgressHandle::default()).is_err());
        }
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let source = root.path().join("file.txt");
    fs::write(&source, b"file").unwrap();
    let target = root.path().join("new.taz");
    assert!(
        operations::add_many_with_progress(&[source], &target, &ProgressHandle::default()).is_err()
    );
    assert!(!target.exists());
}

#[test]
fn cancelling_tar_repack_keeps_original_and_partial_workspace() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("original.txt");
    fs::write(&source, b"original contents").unwrap();
    let path = root.path().join("archive.txz");
    create(&path, &source);
    let original = fs::read(&path).unwrap();
    let payload = root.path().join("payload.bin");
    let mut state = 42u32;
    let bytes: Vec<u8> = (0..16 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(&payload, &bytes).unwrap();
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker_path = path.clone();
    let worker = thread::spawn(move || {
        archive::apply(
            &worker_path,
            &ArchiveOperation::Add {
                sources: vec![payload],
                directory: "/".into(),
            },
            &worker_progress,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let stage = loop {
        let stage = fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".rs7z-tar-")
                    && path.join("updated.tar").is_file()
            });
        if stage.is_some() || worker.is_finished() || Instant::now() >= deadline {
            break stage;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let snapshot = progress.snapshot();
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert!(snapshot.percent >= 60. && snapshot.percent < 100.);
    let stage = stage.expect("operation reached TAR repacking");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read(stage.join("contents/payload.bin")).unwrap(), bytes);
    assert!(stage.join("updated.tar").is_file());
    assert_eq!(archive::read(&path).unwrap().entries.len(), 1);
}

fn compressed_fixture(tar: &Path, path: &Path) {
    let output = rs7z::service::seven_zip_cli::command()
        .expect("official 7zz is required for archive tests")
        .args(["a", "-tgzip", "--"])
        .arg(path)
        .arg(tar)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
fn lzma_and_zstd_decoding_does_not_invoke_system_encoders() {
    const CHILD_ROOT: &str = "RS7Z_CODEC_TEST_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = std::path::PathBuf::from(root);
        for suffix in ["tlz", "tzst"] {
            let path = root.join(format!("archive.{suffix}"));
            let original = fs::read(&path).unwrap();
            assert!(has(&path, "file.txt"));
            operations::test_archive(&path).unwrap();
            let output = root.join(format!("output-{suffix}"));
            operations::extract(&path, &output).unwrap();
            assert_eq!(
                fs::read(output.join("file.txt")).unwrap(),
                b"CLI routing contents"
            );
            let export = root.join(format!("export-{suffix}"));
            fs::create_dir(&export).unwrap();
            apply(
                &path,
                ArchiveOperation::Transfer {
                    entries: vec!["file.txt".into()],
                    destination: ArchiveDestination::FileSystem(export.clone()),
                    move_entries: false,
                },
            );
            assert_eq!(
                fs::read(export.join("file.txt")).unwrap(),
                b"CLI routing contents"
            );
            assert_eq!(fs::read(path).unwrap(), original);
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("file.txt");
    fs::write(&source, b"CLI routing contents").unwrap();
    for suffix in ["tlz", "tzst"] {
        create(&root.path().join(format!("archive.{suffix}")), &source);
    }
    // Give the child unusable system encoders without changing this process's PATH.
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for name in ["xz", "zstd"] {
        let path = bin.join(name);
        fs::write(
            &path,
            b"#!/bin/sh\necho 'System encoder must not be invoked during decoding' >&2\nexit 77\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cli = rs7z::service::seven_zip_cli::command().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lzma_and_zstd_decoding_does_not_invoke_system_encoders",
            "--nocapture",
        ])
        .env(CHILD_ROOT, root.path())
        .env("PATH", bin)
        .env("RS7Z_7ZZ", cli.get_program())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}

#[test]
fn system_tar_codecs_accept_levels_and_reject_truncated_streams_before_extraction() {
    use rs7z::model::archive::CompressionLevel;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("file.txt");
    fs::write(&source, b"system TAR codec contents").unwrap();
    for suffix in ["tlz", "tzst"] {
        for (ix, level) in [
            CompressionLevel::Store,
            CompressionLevel::Fastest,
            CompressionLevel::Fast,
            CompressionLevel::Normal,
            CompressionLevel::Maximum,
            CompressionLevel::Ultra,
        ]
        .into_iter()
        .enumerate()
        {
            let path = root.path().join(format!("archive-{ix}.{suffix}"));
            let mut options = ArchiveOptions::default();
            options.compression_level = Some(level);
            operations::add_many_with_options(
                std::slice::from_ref(&source),
                &path,
                &options,
                &ProgressHandle::default(),
            )
            .unwrap();
            operations::test_archive(&path).unwrap();
            // System-produced streams remain interoperable with the official CLI.
            let cli = rs7z::service::seven_zip_cli::command()
                .unwrap()
                .args([
                    "x",
                    if suffix == "tlz" { "-tlzma" } else { "-tzstd" },
                    "-so",
                    "-bso0",
                    "-bse2",
                    "-bsp0",
                    "--",
                ])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                cli.status.success(),
                "{}",
                String::from_utf8_lossy(&cli.stderr)
            );
            assert!(
                cli.stdout
                    .windows(b"system TAR codec contents".len())
                    .any(|bytes| bytes == b"system TAR codec contents")
            );
            let bytes = fs::read(&path).unwrap();
            fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
            let output = root.path().join(format!("bad-{ix}-{suffix}"));
            assert!(operations::extract(&path, &output).is_err());
            assert!(!output.exists());
        }
    }
}

fn raw_tar(name: &str, kind: u8) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(b"00000000000\0");
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].fill(b' ');
    header[156] = kind;
    if kind == b'1' || kind == b'2' {
        header[157..162].copy_from_slice(b"/tmp/");
    }
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u32 = header.iter().map(|value| *value as u32).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut bytes = header.to_vec();
    bytes.extend([0u8; 1024]);
    bytes
}

#[test]
fn tar_normalizes_dot_prefix_and_rejects_traversal_links_and_invalid_inner_archives() {
    let root = tempfile::tempdir().unwrap();
    for (ix, (name, kind)) in [
        ("./safe.txt", b'0'),
        ("../escape", b'0'),
        ("/absolute", b'0'),
        ("link", b'2'),
        ("hardlink", b'1'),
        ("fifo", b'6'),
    ]
    .into_iter()
    .enumerate()
    {
        let tar = root.path().join(format!("{ix}.tar"));
        fs::write(&tar, raw_tar(name, kind)).unwrap();
        let path = root.path().join(format!("{ix}.tgz"));
        compressed_fixture(&tar, &path);
        if ix == 0 {
            assert_eq!(archive::read(&path).unwrap().entries[0].name, "safe.txt");
            operations::extract(&path, &root.path().join("safe-output")).unwrap();
            assert!(root.path().join("safe-output/safe.txt").is_file());
        } else {
            assert!(archive::read(&path).is_err(), "accepted {name}");
            let output = root.path().join(format!("bad-output-{ix}"));
            assert!(operations::extract(&path, &output).is_err());
            assert!(!output.exists());
        }
    }
    let fake = root.path().join("fake.tar");
    fs::write(&fake, b"not a tar archive").unwrap();
    let path = root.path().join("fake.tgz");
    compressed_fixture(&fake, &path);
    assert!(archive::read(&path).is_err());
    assert!(operations::test_archive(&path).is_err());
}
