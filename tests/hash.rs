use rs7z::service::{
    hash::{self, HashAlgorithm, HashTarget},
    operations::ProgressHandle,
};
use std::path::PathBuf;

#[test]
fn hashes_files_and_archive_entries() {
    let path = PathBuf::from("tests/fixtures/zipx-deflate64.zipx");
    let progress = ProgressHandle::default();
    let file = hash::calculate(
        HashTarget::FileSystem(vec![path.clone()]),
        HashAlgorithm::All,
        &progress,
    )
    .unwrap();
    assert_eq!(file.values.len(), 10);
    assert_eq!(file.values[0], ("CRC32", "0E2CC2E7".into()));
    assert!(file.values.iter().any(|(name, _)| *name == "MD5"));

    let entry = hash::calculate(
        HashTarget::Archive {
            path,
            entries: vec!["item.txt".into()],
            password: None,
        },
        HashAlgorithm::Crc32,
        &progress,
    )
    .unwrap();
    assert_eq!(entry.values, [("CRC32", "89749A40".into())]);
}

#[test]
fn hashes_multiple_archive_entries_with_the_same_name() {
    use std::{fs::File, io::Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("items.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    for (name, contents) in [("first/item.txt", b"one"), ("second/item.txt", b"two")] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();

    let progress = ProgressHandle::default();
    let result = hash::calculate(
        HashTarget::Archive {
            path,
            entries: vec!["first/item.txt".into(), "second/item.txt".into()],
            password: None,
        },
        HashAlgorithm::All,
        &progress,
    )
    .unwrap();
    assert_eq!(result.values.len(), 10);
    assert_eq!(result.values[0].0, "CRC32");
    assert_eq!(result.values[9].0, "SHA3-256");
}

#[test]
fn hashes_encrypted_entry_and_honors_cancellation() {
    use std::{fs::File, io::Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("encrypted.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "secret.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "secret"),
    )
    .unwrap();
    zip.write_all(b"contents").unwrap();
    zip.finish().unwrap();

    let target = || HashTarget::Archive {
        path: path.clone(),
        entries: vec!["secret.txt".into()],
        password: Some("secret".into()),
    };
    let progress = ProgressHandle::default();
    let result = hash::calculate(target(), HashAlgorithm::Md5, &progress).unwrap();
    assert_eq!(result.values.len(), 1);
    assert_eq!(result.values[0].0, "MD5");
    assert_eq!(result.values[0].1.len(), 32);

    progress.cancel();
    assert!(matches!(
        hash::calculate(target(), HashAlgorithm::All, &progress),
        Err(rs7z::model::operations::OperationError::Cancelled)
    ));
}

#[test]
fn hashes_only_the_selected_hidden_file() {
    use std::fs;

    let directory = tempfile::tempdir().unwrap();
    let selected = directory.path().join(".settings");
    let nested = directory.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(&selected, b"selected").unwrap();
    fs::write(nested.join(".settings"), b"different contents").unwrap();

    let result = hash::calculate(
        HashTarget::FileSystem(vec![selected]),
        HashAlgorithm::Crc32,
        &ProgressHandle::default(),
    )
    .unwrap();
    assert_eq!(
        result.values,
        [("CRC32", format!("{:08X}", crc32fast::hash(b"selected")))]
    );
}
