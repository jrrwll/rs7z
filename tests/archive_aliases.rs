use rs7z::{
    model::{
        archive::{ArchiveDestination, ArchiveFormat, ArchiveOperation, ArchiveOptions},
        file::FileKind,
    },
    service::{
        archive, file,
        operations::{self, ProgressHandle},
    },
};
use std::{fs, path::Path, process::Command};

fn apply(path: &Path, operation: ArchiveOperation) {
    archive::apply(path, &operation, &ProgressHandle::default()).unwrap();
}

#[test]
fn zip_aliases_share_creation_browsing_extraction_testing_and_mutations() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"ZIP alias contents").unwrap();
    for suffix in archive::ZIP_EXTENSIONS
        .iter()
        .filter(|extension| **extension != "z01")
    {
        let path = root.path().join(format!("archive.{suffix}"));
        operations::add(&source, &path).unwrap();
        assert_eq!(archive::format(&path), Some(ArchiveFormat::Zip));
        assert_eq!(
            archive::format(&path.with_extension(suffix.to_uppercase())),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(file::read_item(&path).unwrap().kind, FileKind::Archive);
        assert_eq!(
            archive::extraction_directory(&path),
            root.path().join("archive")
        );
        assert_eq!(
            archive::with_format(&path, ArchiveFormat::TarLzma),
            root.path().join("archive.tar.lzma")
        );
        assert_eq!(archive::with_format(&path, ArchiveFormat::Zip), path);
        assert_eq!(archive::read(&path).unwrap().entries[0].name, "item.txt");
        operations::test_archive(&path).unwrap();
        let output = root.path().join(format!("out-{suffix}"));
        operations::extract(&path, &output).unwrap();
        assert_eq!(
            fs::read(output.join("item.txt")).unwrap(),
            b"ZIP alias contents"
        );
        apply(
            &path,
            ArchiveOperation::Rename {
                entry: "item.txt".into(),
                name: "renamed.txt".into(),
            },
        );
        assert_eq!(archive::read(&path).unwrap().entries[0].name, "renamed.txt");
    }
    assert_eq!(
        archive::format(Path::new("backup.z01")),
        Some(ArchiveFormat::Zip)
    );
    assert!(archive::is_read_only(Path::new("backup.z01")));
}

#[test]
fn encrypted_zip_aliases_require_and_validate_passwords() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("item.txt");
    fs::write(&source, b"secret contents").unwrap();
    for (suffix, encryption) in [("jar", "AES256"), ("zipx", "ZipCrypto"), ("apk", "AES256")] {
        let path = root.path().join(format!("secret.{suffix}"));
        let output = rs7z::service::seven_zip_cli::command()
            .expect("official 7zz is required for archive tests")
            .args(["a", "-tzip", "-psecret"])
            .arg(format!("-mem={encryption}"))
            .arg("--")
            .arg(&path)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(archive::read(&path).unwrap().encrypted);
        let destination = root.path().join(format!("out-{suffix}"));
        assert!(operations::extract(&path, &destination).is_err());
        assert!(!destination.exists());
        assert!(operations::test_archive(&path).is_err());
        assert!(archive::verify_password(&path, "wrong", &ProgressHandle::default()).is_err());
        archive::verify_password(&path, "secret", &ProgressHandle::default()).unwrap();
        let mut options = ArchiveOptions::default();
        options.password = Some("secret".into());
        operations::extract_with_options(&path, &destination, &options, &ProgressHandle::default())
            .unwrap();
        assert_eq!(
            fs::read(destination.join("item.txt")).unwrap(),
            b"secret contents"
        );
    }
}

#[test]
fn zipx_codecs_support_cli_entry_export_and_rename() {
    let root = tempfile::tempdir().unwrap();
    let contents = b"ZIPX codec interoperability\n".repeat(100);
    // Fixed external samples exercise decoding independently of the selected CLI's
    // encoders. Official 7zz can decode ZSTD ZIP, but cannot create it.
    for (method, fixture) in [
        (
            "Deflate64",
            include_bytes!("fixtures/zipx-deflate64.zipx").as_slice(),
        ),
        (
            "BZip2",
            include_bytes!("fixtures/zipx-bzip2.zipx").as_slice(),
        ),
        ("LZMA", include_bytes!("fixtures/zipx-lzma.zipx").as_slice()),
        ("PPMd", include_bytes!("fixtures/zipx-ppmd.zipx").as_slice()),
        ("ZSTD", include_bytes!("fixtures/zipx-zstd.zipx").as_slice()),
    ] {
        let path = root.path().join(format!("{method}.zipx"));
        fs::write(&path, fixture).unwrap();
        let compressed = {
            use std::io::Read;
            let mut reader = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
            let mut entry = reader.by_index_raw(0).unwrap();
            let method = entry.compression();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (method, bytes)
        };
        operations::test_archive(&path).unwrap();
        let extracted = root.path().join(format!("extracted-{method}"));
        operations::extract(&path, &extracted).unwrap();
        assert_eq!(fs::read(extracted.join("item.txt")).unwrap(), contents);
        let output = root.path().join(method);
        fs::create_dir(&output).unwrap();
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["item.txt".into()],
                destination: ArchiveDestination::FileSystem(output.clone()),
                move_entries: false,
            },
        );
        assert_eq!(fs::read(output.join("item.txt")).unwrap(), contents);
        apply(
            &path,
            ArchiveOperation::Rename {
                entry: "item.txt".into(),
                name: "renamed.txt".into(),
            },
        );
        operations::test_archive(&path).unwrap();
        assert_eq!(archive::read(&path).unwrap().entries[0].name, "renamed.txt");
        {
            use std::io::Read;
            let mut reader = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
            let mut entry = reader.by_index_raw(0).unwrap();
            assert_eq!(entry.compression(), compressed.0);
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, compressed.1, "{method} was recompressed");
        }
    }
}

#[test]
fn split_zip_opens_from_z01_or_zip_and_never_modifies_volumes() {
    let root = tempfile::tempdir().unwrap();
    let contents = vec![42u8; 180_000];
    fs::write(root.path().join("payload.bin"), &contents).unwrap();
    let output = Command::new("zip")
        .current_dir(root.path())
        .args(["-0", "-s", "64k", "bundle.zip", "payload.bin"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original: Vec<_> = ["bundle.zip", "bundle.z01", "bundle.z02"]
        .map(|name| {
            (
                root.path().join(name),
                fs::read(root.path().join(name)).unwrap(),
            )
        })
        .into();
    for name in ["bundle.zip", "bundle.z01"] {
        let path = root.path().join(name);
        assert!(archive::is_split(&path));
        let index = archive::read(&path).unwrap();
        assert!(index.read_only);
        assert_eq!(index.path, root.path().join("bundle.zip"));
        assert_eq!(index.entries[0].size, contents.len() as u64);
        operations::test_archive(&path).unwrap();
        let output = root.path().join(format!("extract-{name}"));
        operations::extract(&path, &output).unwrap();
        assert_eq!(fs::read(output.join("payload.bin")).unwrap(), contents);
        let export = root.path().join(format!("export-{name}"));
        fs::create_dir(&export).unwrap();
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["payload.bin".into()],
                destination: ArchiveDestination::FileSystem(export.clone()),
                move_entries: false,
            },
        );
        assert_eq!(fs::read(export.join("payload.bin")).unwrap(), contents);
        assert!(
            archive::apply(
                &path,
                &ArchiveOperation::Delete {
                    entries: vec!["payload.bin".into()]
                },
                &ProgressHandle::default()
            )
            .is_err()
        );
        assert!(operations::add(&root.path().join("payload.bin"), &path).is_err());
    }
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    fs::rename(
        root.path().join("bundle.zip"),
        root.path().join("bundle.ZIP"),
    )
    .unwrap();
    fs::rename(
        root.path().join("bundle.z01"),
        root.path().join("bundle.Z01"),
    )
    .unwrap();
    assert_eq!(
        fs::canonicalize(archive::resolve_source(&root.path().join("bundle.Z01")).unwrap())
            .unwrap(),
        fs::canonicalize(root.path().join("bundle.ZIP")).unwrap()
    );
    fs::remove_file(root.path().join("bundle.ZIP")).unwrap();
    assert!(archive::read(&root.path().join("bundle.Z01")).is_err());
}
