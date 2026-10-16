use rs7z::{
    model::{
        archive::{ArchiveDestination, ArchiveOperation},
        operations::OperationError,
        zip::{CompressionLevel, ZipEncryption, ZipOptions},
    },
    service::{archive, operations::ProgressHandle},
};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn fixture(path: &Path) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    zip.set_comment("keep archive comment");
    for (name, content) in [
        ("docs/nested/one.txt", "one"),
        ("docs/two.txt", "two"),
        ("root.txt", "root"),
        ("docs-other.txt", "keep"),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.add_directory("empty/", SimpleFileOptions::default())
        .unwrap();
    zip.add_directory("target/", SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
}

fn content(path: &Path, name: &str) -> String {
    let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut content = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    content
}

fn apply(path: &Path, operation: ArchiveOperation) {
    archive::apply(path, &operation, &ProgressHandle::default()).unwrap();
}

#[test]
fn zip_entry_metadata_reaches_browser_items() {
    let path = Path::new("tests/fixtures/zipx-deflate64.zipx");
    let index = archive::read(path).unwrap();
    let item = archive::children(&index, "").unwrap().remove(0);
    let metadata = item.archive.as_ref().unwrap();
    assert_eq!(metadata.compressed_size, Some(61));
    assert_eq!(metadata.encrypted, Some(false));
    assert_eq!(metadata.crc32, Some(0x8974_9A40));
    assert_eq!(metadata.comment.as_deref(), Some(""));
    assert!(item.modified.is_some());
    assert!(item.created.is_some());
    assert!(item.accessed.is_some());
}

#[test]
fn missing_zip_entry_fields_remain_unknown() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("plain.zip");
    fixture(&path);
    let index = archive::read(&path).unwrap();
    let item = archive::children(&index, "")
        .unwrap()
        .into_iter()
        .find(|item| item.name == "root.txt")
        .unwrap();
    let metadata = item.archive.as_ref().unwrap();
    assert_eq!(metadata.encrypted, Some(false));
    assert_eq!(metadata.comment.as_deref(), Some(""));
    assert!(item.created.is_none());
    assert!(item.accessed.is_none());
}

#[test]
fn mixed_zip_reports_encryption_per_entry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mixed.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "secret.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
    )
    .unwrap();
    zip.write_all(b"private").unwrap();
    zip.start_file("public.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"public").unwrap();
    zip.finish().unwrap();

    let index = archive::read(&path).unwrap();
    assert!(index.encrypted);
    let files = archive::children(&index, "").unwrap();
    let encrypted = |name| {
        files
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .archive
            .as_ref()
            .unwrap()
            .encrypted
    };
    assert_eq!(encrypted("secret.txt"), Some(true));
    assert_eq!(encrypted("public.txt"), Some(false));
}

#[test]
fn aes_entries_survive_rewrite_and_new_entries_use_selected_options() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("encrypted.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    let options = SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret");
    zip.start_file("old.txt", options).unwrap();
    zip.write_all(b"old contents").unwrap();
    zip.finish().unwrap();

    assert!(archive::read(&path).is_ok());
    assert!(archive::read(&path).unwrap().encrypted);
    assert!(archive::verify_password(&path, "incorrect", &ProgressHandle::default()).is_err());
    archive::verify_password(&path, "secret", &ProgressHandle::default()).unwrap();
    let source = root.path().join("new.txt");
    fs::write(&source, b"new contents").unwrap();
    let mut zip_options = ZipOptions::default();
    zip_options.password = Some("secret".into());
    zip_options.zip_encryption = ZipEncryption::Aes256;
    zip_options.compression_level = Some(CompressionLevel::Store);
    archive::apply_with_options(
        &path,
        &ArchiveOperation::Add {
            sources: vec![source],
            directory: "/".into(),
        },
        &zip_options,
        &ProgressHandle::default(),
    )
    .unwrap();

    let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    assert!(archive.by_index_raw(0).unwrap().encrypted());
    assert!(archive.by_index_raw(1).unwrap().encrypted());
    let mut old = archive.by_name_decrypt("old.txt", b"secret").unwrap();
    let mut old_contents = String::new();
    old.read_to_string(&mut old_contents).unwrap();
    assert_eq!(old_contents, "old contents");
    drop(old);
    let mut new = archive.by_name_decrypt("new.txt", b"secret").unwrap();
    let mut new_contents = String::new();
    new.read_to_string(&mut new_contents).unwrap();
    assert_eq!(new_contents, "new contents");
    assert_eq!(new.compression(), zip::CompressionMethod::Stored);
}

#[test]
fn zipcrypto_archives_are_supported() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    fs::write(&source, b"legacy encryption").unwrap();
    let archive_path = root.path().join("zipcrypto.zip");
    let status = rs7z::service::seven_zip_cli::command()
        .expect("official 7zz is required for archive tests")
        .args(["a", "-tzip", "-psecret", "-mem=ZipCrypto"])
        .arg(&archive_path)
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(archive::read(&archive_path).is_ok());
    assert!(archive::read(&archive_path).unwrap().encrypted);
    assert!(
        archive::verify_password(&archive_path, "incorrect", &ProgressHandle::default()).is_err()
    );
    archive::verify_password(&archive_path, "secret", &ProgressHandle::default()).unwrap();
    let mut options = ZipOptions::default();
    options.password = Some("secret".into());
    archive::apply_with_options(
        &archive_path,
        &ArchiveOperation::Rename {
            entry: "source.txt".into(),
            name: "renamed.txt".into(),
        },
        &options,
        &ProgressHandle::default(),
    )
    .unwrap();
    let mut archive = ZipArchive::new(File::open(&archive_path).unwrap()).unwrap();
    // Direct CLI editing retains ZipCrypto instead of upgrading existing entries.
    assert!(
        archive
            .get_aes_verification_key_and_salt(0)
            .unwrap()
            .is_none()
    );
    let mut contents = String::new();
    archive
        .by_name_decrypt("renamed.txt", b"secret")
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();
    assert_eq!(contents, "legacy encryption");
}

#[test]
fn new_zip_uses_zipcrypt_by_default_and_supports_aes256() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("secret.txt");
    fs::write(&source, b"secret contents").unwrap();

    for (name, method) in [
        ("default.zip", ZipEncryption::ZipCrypt),
        ("aes.zip", ZipEncryption::Aes256),
    ] {
        let path = root.path().join(name);
        let mut options = ZipOptions::default();
        options.password = Some("password".into());
        if method == ZipEncryption::Aes256 {
            options.zip_encryption = method;
        }
        rs7z::service::operations::add_many_with_options(
            std::slice::from_ref(&source),
            &path,
            &options,
            &ProgressHandle::default(),
        )
        .unwrap();

        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let aes = archive
            .get_aes_verification_key_and_salt(0)
            .unwrap()
            .is_some();
        assert_eq!(aes, method == ZipEncryption::Aes256);
        let mut contents = String::new();
        archive
            .by_name_decrypt("secret.txt", b"password")
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert_eq!(contents, "secret contents");
    }
}

#[test]
fn password_verification_authenticates_all_encrypted_entries_without_extracting() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("protected.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("plain.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"public").unwrap();
    for (name, password) in [("first.txt", "first"), ("nested/second.txt", "second")] {
        zip.start_file(
            name,
            SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, password),
        )
        .unwrap();
        zip.write_all(b"private contents").unwrap();
    }
    zip.finish().unwrap();
    let original = fs::read(&path).unwrap();
    let before: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(archive::read(&path).unwrap().encrypted);
    // Validating only the first encrypted member would incorrectly accept this password.
    assert!(archive::verify_password(&path, "first", &ProgressHandle::default()).is_err());
    assert!(archive::verify_password(&path, "second", &ProgressHandle::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>(),
        before
    );
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert!(matches!(
        archive::verify_password(&path, "first", &cancelled),
        Err(OperationError::Cancelled)
    ));

    // Damage the AES authentication tag: metadata and password header still remain valid.
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "first.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "first"),
    )
    .unwrap();
    zip.write_all(b"authenticated contents").unwrap();
    zip.finish().unwrap();
    archive::verify_password(&path, "first", &ProgressHandle::default()).unwrap();
    let original = fs::read(&path).unwrap();
    let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let entry = archive.by_index_raw(0).unwrap();
    let data_start = entry
        .data_start()
        .expect("written entry has a known data start");
    let tag_end = (data_start + entry.compressed_size() - 1) as usize;
    drop(entry);
    drop(archive);
    let mut damaged = original;
    damaged[tag_end] ^= 1;
    fs::write(&path, &damaged).unwrap();
    assert!(archive::read(&path).unwrap().encrypted);
    assert!(archive::verify_password(&path, "first", &ProgressHandle::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), damaged);
}

#[test]
fn browse_implicit_directories_and_empty_root() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("browse.zip");
    fixture(&path);
    let index = archive::read(&path).unwrap();
    assert!(!index.encrypted);
    let entries = archive::children(&index, "").unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["docs", "empty", "target", "docs-other.txt", "root.txt"]
    );
    assert_eq!(
        archive::children(&index, "docs/nested").unwrap()[0].name,
        "one.txt"
    );
    assert!(archive::children(&index, "empty").unwrap().is_empty());
    assert!(archive::children(&index, "missing").is_err());
    assert_eq!(archive::directories(&index)[0], "");
    assert_eq!(
        archive::archive_path(&path.join("docs")),
        Some(path.clone())
    );
    assert_eq!(archive::archive_path(&path.join("")), Some(path.clone()));
    let empty = root.path().join("empty.zip");
    ZipWriter::new(File::create(&empty).unwrap())
        .finish()
        .unwrap();
    assert!(
        archive::children(&archive::read(&empty).unwrap(), "/")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn add_mixed_real_sources_to_subdirectory_and_root() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("add.zip");
    fixture(&path);
    let sources = root.path().join("sources");
    fs::create_dir(&sources).unwrap();
    let folder = sources.join("资料");
    fs::create_dir(&folder).unwrap();
    fs::create_dir(folder.join("empty")).unwrap();
    fs::write(folder.join("a [1].txt"), "data").unwrap();
    let file = sources.join("file.txt");
    fs::write(&file, "file").unwrap();
    apply(
        &path,
        ArchiveOperation::Add {
            sources: vec![folder, file.clone()],
            directory: "docs".into(),
        },
    );
    assert_eq!(content(&path, "docs/资料/a [1].txt"), "data");
    assert_eq!(content(&path, "docs/file.txt"), "file");
    apply(
        &path,
        ArchiveOperation::Add {
            sources: vec![file],
            directory: "/".into(),
        },
    );
    assert_eq!(content(&path, "file.txt"), "file");
    assert_eq!(content(&path, "root.txt"), "root");
    assert_eq!(
        ZipArchive::new(File::open(&path).unwrap())
            .unwrap()
            .comment(),
        b"keep archive comment"
    );
}

#[test]
fn copy_and_move_batches_inside_archive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transfer.zip");
    fixture(&path);
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec![
                "docs".into(),
                "docs/two.txt".into(),
                "empty".into(),
                "root.txt".into(),
            ],
            destination: ArchiveDestination::Directory("target".into()),
            move_entries: false,
        },
    );
    assert_eq!(content(&path, "target/docs/nested/one.txt"), "one");
    assert_eq!(content(&path, "docs/nested/one.txt"), "one");
    assert!(archive::directories(&archive::read(&path).unwrap()).contains(&"target/empty".into()));
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["target/docs/two.txt".into(), "target/docs/nested".into()],
            destination: ArchiveDestination::Directory("/".into()),
            move_entries: true,
        },
    );
    assert_eq!(content(&path, "two.txt"), "two");
    assert_eq!(content(&path, "nested/one.txt"), "one");
    assert!(
        archive::children(&archive::read(&path).unwrap(), "target/docs")
            .unwrap()
            .is_empty()
    );
    apply(
        &path,
        ArchiveOperation::Delete {
            entries: vec!["docs".into(), "target/root.txt".into()],
        },
    );
    let index = archive::read(&path).unwrap();
    assert!(
        !index
            .entries
            .iter()
            .any(|e| e.name == "docs" || e.name.starts_with("docs/"))
    );
    assert_eq!(content(&path, "docs-other.txt"), "keep");
}

#[test]
fn copy_and_move_batches_to_system_folder() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("export.zip");
    fixture(&path);
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs".into(), "empty".into(), "root.txt".into()],
            destination: ArchiveDestination::FileSystem(output.clone()),
            move_entries: false,
        },
    );
    assert_eq!(
        fs::read(output.join("docs/nested/one.txt")).unwrap(),
        b"one"
    );
    assert!(output.join("empty").is_dir());
    assert_eq!(content(&path, "root.txt"), "root");
    let moved = root.path().join("moved");
    fs::create_dir(&moved).unwrap();
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs/nested".into(), "root.txt".into()],
            destination: ArchiveDestination::FileSystem(moved.clone()),
            move_entries: true,
        },
    );
    assert_eq!(fs::read(moved.join("nested/one.txt")).unwrap(), b"one");
    assert!(
        !archive::read(&path)
            .unwrap()
            .entries
            .iter()
            .any(|e| e.name == "root.txt" || e.name.starts_with("docs/nested"))
    );
    assert_eq!(content(&path, "docs/two.txt"), "two");
}

#[test]
fn reject_conflicts_descendants_and_cancel_without_changing_archive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("safe.zip");
    fixture(&path);
    let original = fs::read(&path).unwrap();
    for directory in ["/", "docs", "docs/nested", "docs/new", "../escape"] {
        let operation = ArchiveOperation::Transfer {
            entries: vec!["docs".into()],
            destination: ArchiveDestination::Directory(directory.into()),
            move_entries: true,
        };
        assert!(archive::apply(&path, &operation, &ProgressHandle::default()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let operation = ArchiveOperation::Delete {
        entries: vec!["docs".into()],
    };
    let progress = ProgressHandle::default();
    progress.cancel();
    assert_eq!(
        archive::apply(&path, &operation, &progress),
        Err(OperationError::Cancelled)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("root.txt"), "existing").unwrap();
    let operation = ArchiveOperation::Transfer {
        entries: vec!["docs".into(), "root.txt".into()],
        destination: ArchiveDestination::FileSystem(output.clone()),
        move_entries: true,
    };
    assert!(archive::apply(&path, &operation, &ProgressHandle::default()).is_err());
    assert!(!output.join("docs").exists());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn add_copy_and_move_create_nested_destinations() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new-directories.zip");
    fixture(&path);
    let file = root.path().join("added.txt");
    fs::write(&file, "added").unwrap();
    let destination = "资料 café 123.-_+=@#/new folder";
    let index = archive::read(&path).unwrap();
    for input in [
        destination.to_owned(),
        format!("/{destination}"),
        format!("/{destination}/"),
    ] {
        assert_eq!(
            archive::destination_directory(&index, &input).unwrap(),
            destination
        );
    }
    assert_eq!(archive::destination_directory(&index, "/").unwrap(), "");
    apply(
        &path,
        ArchiveOperation::Add {
            sources: vec![file],
            directory: destination.into(),
        },
    );
    assert_eq!(content(&path, &format!("{destination}/added.txt")), "added");
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs".into(), "empty".into(), "root.txt".into()],
            destination: ArchiveDestination::Directory("/copies/nested/".into()),
            move_entries: false,
        },
    );
    assert_eq!(content(&path, "copies/nested/docs/nested/one.txt"), "one");
    assert_eq!(content(&path, "docs/nested/one.txt"), "one");
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["docs".into(), "root.txt".into()],
            destination: ArchiveDestination::Directory("moves/new".into()),
            move_entries: true,
        },
    );
    assert_eq!(content(&path, "moves/new/docs/nested/one.txt"), "one");
    assert_eq!(content(&path, "moves/new/root.txt"), "root");
    let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    assert!(zip.by_name("root.txt").is_err());
    assert!(zip.by_name("docs/nested/one.txt").is_err());
    for directory in [
        "资料 café 123.-_+=@#/",
        "资料 café 123.-_+=@#/new folder/",
        "copies/",
        "copies/nested/",
        "copies/nested/empty/",
        "moves/",
        "moves/new/",
    ] {
        assert!(zip.by_name(directory).unwrap().is_dir(), "{directory}");
    }
}

#[test]
fn invalid_destinations_leave_archive_unchanged_for_all_operations() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("invalid-destinations.zip");
    fixture(&path);
    let source = root.path().join("added.txt");
    fs::write(&source, "added").unwrap();
    let original = fs::read(&path).unwrap();
    for directory in [
        "",
        " ",
        ".",
        "..",
        "...",
        "....",
        "//",
        "//new",
        "new//",
        "new//child",
        "new/ /child",
        "new/./child",
        "new/../child",
        "new/.../child",
        "new\\child",
        "new:child",
        "new*",
        "new?",
        "new!",
        "new[1]",
        "new$",
        "new%",
        "new&",
        "new;",
        "new'",
        "new\"",
        "new`",
        "new<",
        "new>",
        "new|",
        "new\tchild",
        "new\nchild",
        "new\0child",
        "new😀",
        "root.txt",
        "root.txt/child",
    ] {
        for operation in [
            ArchiveOperation::Add {
                sources: vec![source.clone()],
                directory: directory.into(),
            },
            ArchiveOperation::Transfer {
                entries: vec!["docs".into()],
                destination: ArchiveDestination::Directory(directory.into()),
                move_entries: false,
            },
            ArchiveOperation::Transfer {
                entries: vec!["docs".into()],
                destination: ArchiveDestination::Directory(directory.into()),
                move_entries: true,
            },
        ] {
            assert!(
                archive::apply(&path, &operation, &ProgressHandle::default()).is_err(),
                "accepted {directory:?}"
            );
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn existing_directories_with_other_characters_remain_usable() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("existing-names.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file("file.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"contents").unwrap();
    for directory in ["old [v1]!/", ".../", " /", "emoji😀/"] {
        zip.add_directory(directory, SimpleFileOptions::default())
            .unwrap();
    }
    zip.finish().unwrap();
    for directory in ["old [v1]!", "...", " ", "emoji😀"] {
        apply(
            &path,
            ArchiveOperation::Transfer {
                entries: vec!["file.txt".into()],
                destination: ArchiveDestination::Directory(format!("/{directory}")),
                move_entries: false,
            },
        );
        assert_eq!(content(&path, &format!("{directory}/file.txt")), "contents");
    }
}

#[test]
fn rename_archive_files_and_directories_preserves_contents() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rename.zip");
    fixture(&path);
    let original = fs::read(&path).unwrap();
    for name in [
        "",
        " ",
        ".",
        "..",
        "...",
        "../escape",
        "new/child",
        "bad\\name",
        "root.txt",
        "docs",
    ] {
        let operation = ArchiveOperation::Rename {
            entry: "root.txt".into(),
            name: name.into(),
        };
        assert!(
            archive::apply(&path, &operation, &ProgressHandle::default()).is_err(),
            "accepted {name:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    let operation = ArchiveOperation::Rename {
        entry: "docs".into(),
        name: "new docs".into(),
    };
    assert_eq!(
        archive::apply(&path, &operation, &cancelled),
        Err(OperationError::Cancelled)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    apply(&path, operation);
    assert_eq!(content(&path, "new docs/nested/one.txt"), "one");
    assert_eq!(content(&path, "new docs/two.txt"), "two");
    assert_eq!(content(&path, "docs-other.txt"), "keep");
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: "new docs/two.txt".into(),
            name: "新名字 [2].txt".into(),
        },
    );
    assert_eq!(content(&path, "new docs/新名字 [2].txt"), "two");
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: "empty".into(),
            name: "new empty".into(),
        },
    );
    let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    assert!(zip.by_name("docs/nested/one.txt").is_err());
    assert!(zip.by_name("new docs/two.txt").is_err());
    assert!(zip.by_name("empty/").is_err());
    assert!(zip.by_name("new empty/").unwrap().is_dir());
    assert_eq!(zip.comment(), b"keep archive comment");
}

#[test]
fn reject_archive_traversal_paths() {
    let root = tempfile::tempdir().unwrap();
    for (ix, name) in ["../escape", "/absolute", "dir/../../escape", "dir\\escape"]
        .iter()
        .enumerate()
    {
        let path = root.path().join(format!("unsafe-{ix}.zip"));
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"data").unwrap();
        zip.finish().unwrap();
        assert!(archive::read(&path).is_err(), "accepted {name}");
    }
}

#[test]
fn cancel_rewrite_retains_work_file_and_readable_original() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cancel.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    let block = vec![17u8; 1024 * 1024];
    for ix in 0..64 {
        zip.start_file(
            format!("file-{ix}"),
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&block).unwrap();
    }
    zip.finish().unwrap();
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        archive::apply(
            &worker_path,
            &ArchiveOperation::Delete {
                entries: vec!["file-63".into()],
            },
            &worker_progress,
        )
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while progress.snapshot().percent == 0.
        && !worker.is_finished()
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert_eq!(
        ZipArchive::new(File::open(&path).unwrap()).unwrap().len(),
        64
    );
    assert!(fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".rs7z-zip-")
    }));
    assert_eq!(content(&path, "file-0").len(), block.len());
}

fn raw_entry(path: &Path, name: &str) -> (bool, zip::CompressionMethod, Vec<u8>) {
    let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
    for ix in 0..archive.len() {
        let mut entry = archive.by_index_raw(ix).unwrap();
        if entry.name().unwrap().as_ref() == name {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            return (entry.encrypted(), entry.compression(), bytes);
        }
    }
    panic!("Missing ZIP entry: {name}");
}

#[test]
fn cli_edits_preserve_untouched_payloads_comments_and_mixed_encryption() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mixed.zip");
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    writer.set_comment("unchanged comment");
    writer
        .start_file("plain.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"plain contents").unwrap();
    writer
        .start_file(
            "secret.txt",
            SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
        )
        .unwrap();
    writer.write_all(b"private contents").unwrap();
    writer.finish().unwrap();
    let plain = raw_entry(&path, "plain.txt");
    let secret = raw_entry(&path, "secret.txt");
    // Neither rename nor move needs to decode the original AES data.
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: "secret.txt".into(),
            name: "renamed.txt".into(),
        },
    );
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: vec!["renamed.txt".into()],
            destination: ArchiveDestination::Directory("nested".into()),
            move_entries: true,
        },
    );
    assert_eq!(raw_entry(&path, "nested/renamed.txt"), secret);
    assert_eq!(raw_entry(&path, "plain.txt"), plain);
    let source = root.path().join("new.txt");
    fs::write(&source, b"new contents").unwrap();
    let mut options = ZipOptions::default();
    options.password = Some("secret".into());
    archive::apply_with_options(
        &path,
        &ArchiveOperation::Add {
            sources: vec![source],
            directory: "/".into(),
        },
        &options,
        &ProgressHandle::default(),
    )
    .unwrap();
    assert!(raw_entry(&path, "new.txt").0);
    assert_eq!(raw_entry(&path, "plain.txt"), plain);
    assert_eq!(raw_entry(&path, "nested/renamed.txt"), secret);
    apply(
        &path,
        ArchiveOperation::Delete {
            entries: vec!["new.txt".into()],
        },
    );
    assert_eq!(raw_entry(&path, "nested/renamed.txt"), secret);
    assert_eq!(
        ZipArchive::new(File::open(&path).unwrap())
            .unwrap()
            .comment(),
        b"unchanged comment"
    );
    archive::verify_password(&path, "secret", &ProgressHandle::default()).unwrap();
}

#[test]
fn selected_export_ignores_unselected_corruption_but_password_test_checks_every_entry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("damaged.zip");
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    writer
        .start_file(
            "good.txt",
            SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
        )
        .unwrap();
    writer.write_all(b"good contents").unwrap();
    writer
        .start_file(
            "bad.txt",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(b"damaged contents").unwrap();
    writer.finish().unwrap();
    let mut reader = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let start = reader
        .by_name("bad.txt")
        .unwrap()
        .data_start()
        .expect("written entry has a known data start") as usize;
    drop(reader);
    let mut bytes = fs::read(&path).unwrap();
    bytes[start] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    let mut options = ZipOptions::default();
    options.password = Some("secret".into());
    archive::apply_with_options(
        &path,
        &ArchiveOperation::Transfer {
            entries: vec!["good.txt".into()],
            destination: ArchiveDestination::FileSystem(output.clone()),
            move_entries: false,
        },
        &options,
        &ProgressHandle::default(),
    )
    .unwrap();
    assert_eq!(fs::read(output.join("good.txt")).unwrap(), b"good contents");
    assert!(!output.join("bad.txt").exists());
    assert!(archive::verify_password(&path, "secret", &ProgressHandle::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    // Copying inside the ZIP also decodes only the selected entry.
    archive::apply_with_options(
        &path,
        &ArchiveOperation::Transfer {
            entries: vec!["good.txt".into()],
            destination: ArchiveDestination::Directory("copies".into()),
            move_entries: false,
        },
        &options,
        &ProgressHandle::default(),
    )
    .unwrap();
    apply(
        &path,
        ArchiveOperation::Delete {
            entries: vec!["bad.txt".into()],
        },
    );
    archive::verify_password(&path, "secret", &ProgressHandle::default()).unwrap();
}

#[test]
fn literal_names_and_batches_never_select_neighbors() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("literal.zip");
    let names: Vec<_> = [
        " leading ",
        "@list",
        "-pfile",
        "[file]",
        "Case",
        "case",
        "prefix",
        "prefix-other",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain((0..150).map(|ix| format!("file-{ix:0200}")))
    .collect();
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    for name in &names {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(name.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    apply(
        &path,
        ArchiveOperation::Rename {
            entry: " leading ".into(),
            name: "renamed".into(),
        },
    );
    assert_eq!(content(&path, "renamed"), " leading ");
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    let selected: Vec<_> = names
        .iter()
        .filter(|name| ![" leading ", "case", "prefix-other"].contains(&name.as_str()))
        .cloned()
        .collect();
    apply(
        &path,
        ArchiveOperation::Transfer {
            entries: selected.clone(),
            destination: ArchiveDestination::FileSystem(output.clone()),
            move_entries: true,
        },
    );
    for name in &selected {
        assert_eq!(fs::read(output.join(name)).unwrap(), name.as_bytes());
    }
    assert_eq!(content(&path, "case"), "case");
    assert_eq!(content(&path, "prefix-other"), "prefix-other");
    assert_eq!(archive::read(&path).unwrap().entries.len(), 3);
}

#[test]
fn deleting_every_entry_leaves_a_valid_empty_zip() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("empty.zip");
    fixture(&path);
    let entries = archive::children(&archive::read(&path).unwrap(), "/")
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    apply(&path, ArchiveOperation::Delete { entries });
    let reader = ZipArchive::new(File::open(&path).unwrap()).unwrap();
    assert!(reader.is_empty());
    assert_eq!(reader.comment(), b"keep archive comment");
    assert!(archive::read(&path).unwrap().entries.is_empty());
}

#[test]
fn export_rejects_host_filename_collisions_before_moving_entries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("probe"), b"probe").unwrap();
    if !root.path().join("PROBE").exists() {
        return;
    }
    let path = root.path().join("collision.zip");
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    for name in ["a.txt", "A.txt"] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(name.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    let original = fs::read(&path).unwrap();
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    assert!(
        archive::apply(
            &path,
            &ArchiveOperation::Transfer {
                entries: vec!["a.txt".into(), "A.txt".into()],
                destination: ArchiveDestination::FileSystem(output.clone()),
                move_entries: true
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read_dir(output).unwrap().count(), 0);

    // Different source directories can also flatten to the same destination name.
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    for name in ["left/a.txt", "right/A.txt"] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(name.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    let original = fs::read(&path).unwrap();
    assert!(
        archive::apply(
            &path,
            &ArchiveOperation::Transfer {
                entries: vec!["left/a.txt".into(), "right/A.txt".into()],
                destination: ArchiveDestination::Directory("copies".into()),
                move_entries: false,
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), original);

    // Retaining two implicit directories must never silently merge their names.
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    for name in ["dir/a.txt", "DIR/b.txt"] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(name.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    let original = fs::read(&path).unwrap();
    assert!(
        archive::apply(
            &path,
            &ArchiveOperation::Delete {
                entries: vec!["dir/a.txt".into(), "DIR/b.txt".into()],
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), original);
}
