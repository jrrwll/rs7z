use rs7z::{
    model::{
        archive::{ArchiveFormat, ArchiveOperation, ArchiveOptions},
        operations::OperationError,
    },
    service::{
        archive,
        operations::{self, ProgressHandle},
    },
};
use std::fs;

// Minimal stored RAR4 records let failure/cancellation tests control the exact
// payload without depending on a proprietary RAR encoder or a large fixture.
fn stored_rar(path: &std::path::Path, entries: &[(&str, &[u8], bool)]) {
    use std::io::Write;
    fn header(output: &mut fs::File, kind: u8, flags: u16, body: &[u8]) {
        let mut bytes = vec![kind];
        bytes.extend(flags.to_le_bytes());
        bytes.extend((7u16 + body.len() as u16).to_le_bytes());
        bytes.extend(body);
        output
            .write_all(&(crc32fast::hash(&bytes) as u16).to_le_bytes())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    let mut output = fs::File::create(path).unwrap();
    output.write_all(b"Rar!\x1a\x07\x00").unwrap();
    header(&mut output, 0x73, 0, &[0; 6]);
    for (name, contents, corrupt) in entries {
        let mut body = Vec::new();
        body.extend((contents.len() as u32).to_le_bytes());
        body.extend((contents.len() as u32).to_le_bytes());
        body.push(3); // Unix
        let crc = crc32fast::hash(contents) ^ u32::from(*corrupt);
        body.extend(crc.to_le_bytes());
        body.extend(0x58210000u32.to_le_bytes()); // 2024-01-01, DOS timestamp
        body.extend([20, 0x30]); // RAR 2.0, stored
        body.extend((name.len() as u16).to_le_bytes());
        body.extend(0o100644u32.to_le_bytes());
        body.extend(name.as_bytes());
        header(&mut output, 0x74, 0x8000, &body);
        output.write_all(contents).unwrap();
    }
    header(&mut output, 0x7b, 0, &[]);
}

#[test]
fn rar_crc_failure_removes_only_the_failed_file_and_rejects_unsafe_paths() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("damaged.rar");
    stored_rar(
        &archive,
        &[
            ("good.txt", b"complete contents", false),
            ("bad.txt", b"damaged contents", true),
            ("last.txt", b"later contents", false),
        ],
    );
    let output = root.path().join("output");
    let error = operations::extract(&archive, &output).unwrap_err();
    assert!(output.join("good.txt").exists(), "{error}");
    assert_eq!(
        fs::read(output.join("good.txt")).unwrap(),
        b"complete contents"
    );
    assert_eq!(
        fs::read(output.join("last.txt")).unwrap(),
        b"later contents"
    );
    assert!(!output.join("bad.txt").exists());
    stored_rar(&archive, &[("../escape.txt", b"unsafe", false)]);
    let output = root.path().join("unsafe-output");
    assert!(operations::extract(&archive, &output).is_err());
    assert!(!output.exists());
    assert!(!root.path().join("escape.txt").exists());
}

#[test]
fn rar_cancellation_keeps_output_without_requiring_percentage_updates() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("large.rar");
    let contents = vec![42; 64 * 1024 * 1024];
    stored_rar(&archive, &[("large.bin", &contents, false)]);
    let output = root.path().join("output");
    let progress = ProgressHandle::default();
    let worker_progress = progress.clone();
    let worker_output = output.clone();
    let worker = thread::spawn(move || {
        operations::extract_with_progress(&archive, &worker_output, &worker_progress)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output.join("large.bin").exists() && !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_micros(100));
    }
    progress.cancel();
    assert_eq!(worker.join().unwrap(), Err(OperationError::Cancelled));
    assert!(progress.output().unwrap().contains("Extracting archive"));
    let partial = fs::read(output.join("large.bin")).unwrap();
    assert!(partial.len() <= contents.len());
    assert_eq!(&partial, &contents[..partial.len()]);
}

#[test]
fn rar_extracts_contents_but_rejects_creation_testing_and_mutation() {
    let root = tempfile::tempdir().unwrap();
    let bytes = include_bytes!("fixtures/sample-rar5.rar");
    let entry = "helloworld.txt";
    let path = root.path().join("sample.RAR");
    fs::write(&path, bytes).unwrap();
    assert_eq!(archive::format(&path), Some(ArchiveFormat::Rar));
    assert!(archive::is_extract_only(&path));
    assert!(archive::read(&path).unwrap().read_only);
    let output = root.path().join("output");
    let progress = ProgressHandle::default();
    operations::extract_with_progress(&path, &output, &progress).unwrap();
    let log = progress
        .output()
        .expect("RAR extraction streams CLI output");
    assert!(log.contains("Everything is Ok"), "{log}");
    assert!(log.contains("helloworld.txt"), "{log}");
    assert!(!log.contains(['\x08', '\r']));
    assert_eq!(
        fs::read(output.join(entry)).unwrap(),
        b"hello libarchive test suite!\n"
    );
    assert!(operations::test_archive(&path).is_err());
    assert!(
        archive::apply(
            &path,
            &ArchiveOperation::Delete {
                entries: vec![entry.into()]
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert!(operations::add(&output.join(entry), &root.path().join("new.rar")).is_err());
    assert!(!root.path().join("new.rar").exists());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn rar_solid_and_multiple_volumes_extract_and_missing_volume_fails() {
    let root = tempfile::tempdir().unwrap();
    let solid = root.path().join("solid.rar");
    fs::write(&solid, include_bytes!("fixtures/rar5-solid.rar")).unwrap();
    let output = root.path().join("solid-output");
    operations::extract(&solid, &output).unwrap();
    for (index, crc) in [0x7e13b2c6, 0xf166afcb, 0x9fb123d9, 0x10c43ed4]
        .into_iter()
        .enumerate()
    {
        let contents = fs::read(output.join(format!("test{}.bin", index + 1))).unwrap();
        assert_eq!(contents.len(), 4096);
        assert_eq!(crc32fast::hash(&contents), crc);
    }
    for (index, bytes) in [
        include_bytes!("fixtures/rar4-volume.part1.rar").as_slice(),
        include_bytes!("fixtures/rar4-volume.part2.rar").as_slice(),
        include_bytes!("fixtures/rar4-volume.part3.rar").as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        fs::write(
            root.path().join(format!("volume.part{}.rar", index + 1)),
            bytes,
        )
        .unwrap();
    }
    let first = root.path().join("volume.part1.rar");
    let output = root.path().join("volume-output");
    operations::extract(&first, &output).unwrap();
    let contents = fs::read(output.join("LibarchiveAddingTest.html")).unwrap();
    assert_eq!(contents.len(), 20111);
    assert_eq!(crc32fast::hash(&contents), 0x5e05a663);
    fs::remove_file(root.path().join("volume.part3.rar")).unwrap();
    let missing = root.path().join("missing-output");
    let error = operations::extract(&first, &missing).unwrap_err();
    assert!(error.to_ascii_lowercase().contains("volume"), "{error}");
}

#[test]
fn rar_links_are_rejected_before_creating_output() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("links.rar");
    fs::write(&path, include_bytes!("fixtures/sample-rar4.rar")).unwrap();
    let output = root.path().join("output");
    let error = operations::extract(&path, &output).unwrap_err();
    assert!(error.contains("Links or special files"), "{error}");
    assert!(!output.exists());
}

#[test]
fn rar_encrypted_headers_require_a_password_before_creating_outputs() {
    let root = tempfile::tempdir().unwrap();
    for (ix, bytes) in [
        include_bytes!("fixtures/rar4-encrypted-headers.rar").as_slice(),
        include_bytes!("fixtures/rar5-encrypted-headers.rar").as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = root.path().join(format!("encrypted-{ix}.rar"));
        fs::write(&path, bytes).unwrap();
        let output = root.path().join(format!("output-{ix}"));
        assert!(archive::read(&path).unwrap().encrypted);
        assert!(operations::extract(&path, &output).is_err());
        assert!(!output.exists());
        let mut options = ArchiveOptions::default();
        options.password = Some("wrong".into());
        assert!(
            operations::extract_with_options(&path, &output, &options, &ProgressHandle::default())
                .is_err()
        );
        assert!(!output.exists());
        assert!(archive::verify_password(&path, "wrong", &ProgressHandle::default()).is_err());
        archive::verify_password(&path, "password", &ProgressHandle::default()).unwrap();
        options.password = Some("password".into());
        operations::extract_with_options(&path, &output, &options, &ProgressHandle::default())
            .unwrap();
        for name in ["a", "b", "c", "d"] {
            assert_eq!(
                fs::read(output.join(format!("{name}.txt"))).unwrap(),
                format!("This is from {name}.txt").as_bytes()
            );
        }
        let progress = ProgressHandle::default();
        progress.cancel();
        assert_eq!(
            operations::extract_with_options(
                &path,
                &root.path().join("cancelled"),
                &options,
                &progress
            ),
            Err(OperationError::Cancelled)
        );
        assert!(!root.path().join("cancelled").exists());
    }
}
