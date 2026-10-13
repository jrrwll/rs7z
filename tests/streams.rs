use rs7z::{
    model::{
        archive::{ArchiveFormat, ArchiveOperation, ArchiveOptions, StreamFormat},
        operations::OperationError,
    },
    service::{
        archive,
        operations::{self, ProgressHandle},
        seven_zip_cli,
    },
};
use std::{fs, path::Path, process::Command};

fn encoded(codec: &str, payload: &[u8]) -> Vec<u8> {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("embedded-name.txt");
    let output = root.path().join("encoded");
    fs::write(&input, payload).unwrap();
    let mut command = seven_zip_cli::command().unwrap();
    let result = command
        .args(["a", codec, "-sae", "--"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::read(output).unwrap()
}

fn system_encoded(program: &str, args: &[&str], payload: &[u8]) -> Vec<u8> {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input");
    fs::write(&input, payload).unwrap();
    let result = Command::new(program)
        .args(args)
        .arg(input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn extract(source: &Path, destination: &Path) -> Result<(), OperationError> {
    operations::extract_with_progress(source, destination, &ProgressHandle::default())
}

#[test]
fn standalone_suffixes_are_extract_only_and_tar_suffixes_take_precedence() {
    for (format, suffixes) in [
        (StreamFormat::Gzip, vec!["gz", "gzip"]),
        (StreamFormat::Bzip2, vec!["bz2", "bzip2"]),
        (StreamFormat::Xz, vec!["xz"]),
        (StreamFormat::Zstd, vec!["zst", "zstd"]),
        (StreamFormat::Lzma, vec!["lzma"]),
        (StreamFormat::Compress, vec!["Z"]),
    ] {
        for suffix in suffixes {
            for suffix in [suffix.to_owned(), suffix.to_uppercase()] {
                let path = Path::new("/tmp").join(format!("文件.txt.{suffix}"));
                assert_eq!(archive::format(&path), Some(ArchiveFormat::Stream(format)));
                assert_eq!(archive::stream_output_name(&path).unwrap(), "文件.txt");
                assert!(archive::is_extract_only(&path));
                assert!(archive::is_read_only(&path));
                assert!(!archive::supports_password(ArchiveFormat::Stream(format)));
                assert!(
                    archive::read(&path)
                        .unwrap_err()
                        .contains("extraction only")
                );
                assert!(
                    operations::test_archive_with_progress(&path, &ProgressHandle::default())
                        .is_err()
                );
            }
        }
    }
    for name in [
        "a.tar.gz",
        "a.tgz",
        "a.tar.bz2",
        "a.tbz2",
        "a.tar.xz",
        "a.txz",
        "a.tar.lzma",
        "a.tlz",
        "a.tar.zst",
        "a.tzst",
        "a.tar.Z",
        "a.taz",
    ] {
        let path = Path::new(name);
        assert!(archive::format(path).is_some_and(archive::is_tar));
        assert!(!archive::is_extract_only(path));
    }
    for name in [
        "a.lzma86",
        "a.b64",
        "a.base64",
        "a.tar",
        "a.tar.lzma86",
        "a.tar.b64",
        "a.tar.base64",
        "a.tar.gzip",
        "a.tar.zstd",
    ] {
        for name in [name.to_owned(), name.to_uppercase()] {
            let path = Path::new(&name);
            assert!(archive::format(path).is_none());
            assert!(!archive::is_extract_only(path));
        }
    }
    for extension in ["lzma86", "b64", "base64"] {
        assert!(!archive::OPEN_EXTENSIONS.contains(&extension));
    }
    for name in [".gz", "..gz", "...gz"] {
        assert!(archive::stream_output_name(Path::new(name)).is_err());
    }
}

#[test]
fn all_six_stream_codecs_decode_one_layer_using_the_source_name() {
    let root = tempfile::tempdir().unwrap();
    let payload = b"hello\0\xff\nstandalone stream\n";
    let lzma = system_encoded("xz", &["--format=lzma", "-c"], payload);
    for (suffix, data) in [
        ("GZIP", encoded("-tgzip", payload)),
        ("bzip2", encoded("-tbzip2", payload)),
        ("xz", encoded("-txz", payload)),
        ("zstd", system_encoded("zstd", &["-q", "-c"], payload)),
        ("lzma", lzma),
        ("Z", include_bytes!("fixtures/sample.tar.Z").to_vec()),
    ] {
        let source = root.path().join(format!("文件 [1].txt.{suffix}"));
        fs::write(&source, &data).unwrap();
        let destination = root.path().join(suffix).join("output");
        extract(&source, &destination).unwrap();
        let decoded = fs::read(destination.join("文件 [1].txt")).unwrap();
        if suffix == "Z" {
            assert_eq!(decoded.len(), 6144);
            assert_eq!(&decoded[257..262], b"ustar");
        } else {
            assert_eq!(decoded, payload, "{suffix}");
        }
        assert_eq!(fs::read(source).unwrap(), data);
        assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
    }
}

#[test]
fn streams_do_not_unpack_tar_or_another_compressed_layer() {
    let root = tempfile::tempdir().unwrap();
    let data = encoded("-tgzip", b"inner payload");
    let source = root.path().join("nested.gz.gz");
    fs::write(&source, encoded("-tgzip", &data)).unwrap();
    extract(&source, root.path()).unwrap();
    assert_eq!(fs::read(root.path().join("nested.gz")).unwrap(), data);
    assert!(!root.path().join("nested").exists());
    // The bytes are TAR, but the filename requests standalone gzip extraction.
    let tar = root.path().join("bundle.tar.gz");
    let input = root.path().join("input.txt");
    fs::write(&input, b"tar content").unwrap();
    operations::add_many_with_progress(&[input], &tar, &ProgressHandle::default()).unwrap();
    let source = root.path().join("renamed.gz");
    fs::rename(tar, &source).unwrap();
    let destination = root.path().join("decoded");
    extract(&source, &destination).unwrap();
    let bytes = fs::read(destination.join("renamed")).unwrap();
    assert_eq!(&bytes[..9], b"input.txt");
    assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
}

#[test]
fn output_conflicts_invalid_names_and_unsupported_operations_write_nothing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("report.gz");
    fs::write(&source, encoded("-tgzip", b"hello")).unwrap();
    let output = root.path().join("report");
    fs::write(&output, b"existing").unwrap();
    assert!(extract(&source, root.path()).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    fs::remove_file(&output).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("missing"), &output).unwrap();
        assert!(extract(&source, root.path()).is_err());
        assert!(output.symlink_metadata().unwrap().is_symlink());
        fs::remove_file(output).unwrap();
    }
    let destination = root.path().join("unused");
    let mut options = ArchiveOptions::default();
    options.password = Some("unused password".into());
    assert!(
        operations::extract_with_options(
            &source,
            &destination,
            &options,
            &ProgressHandle::default()
        )
        .is_err()
    );
    let cancelled = ProgressHandle::default();
    cancelled.cancel();
    assert_eq!(
        operations::extract_with_progress(&source, &destination, &cancelled),
        Err(OperationError::Cancelled)
    );
    assert!(
        operations::add_many_with_progress(
            std::slice::from_ref(&source),
            &root.path().join("new.gz"),
            &ProgressHandle::default()
        )
        .is_err()
    );
    assert!(
        archive::apply(
            &source,
            &ArchiveOperation::Delete {
                entries: vec!["report".into()]
            },
            &ProgressHandle::default()
        )
        .is_err()
    );
    for name in [
        ".gz",
        "archive.lzma86",
        "archive.b64",
        "archive.base64",
        "archive.tar.lzma86",
        "archive.tar.b64",
        "archive.tar",
    ] {
        let invalid = root.path().join(name);
        fs::write(&invalid, b"fixture").unwrap();
        assert!(extract(&invalid, &destination).is_err());
    }
    assert!(!destination.exists());
    assert!(!root.path().join("new.gz").exists());
}

#[test]
fn corrupt_streams_keep_partial_with_a_copyable_error_path() {
    let root = tempfile::tempdir().unwrap();
    let mut corrupt = encoded("-tgzip", b"recoverable partial bytes");
    let crc_offset = corrupt.len() - 8;
    corrupt[crc_offset] ^= 0xff;
    let source = root.path().join("bad.gz");
    fs::write(&source, corrupt).unwrap();
    let destination = root.path().join("output");
    let error = extract(&source, &destination).unwrap_err();
    let OperationError::Failed(error) = error else {
        panic!("expected decode error")
    };
    let partial = fs::read_dir(&destination)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(partial.extension().unwrap(), "partial");
    assert!(error.contains(partial.to_str().unwrap()));
    assert!(!destination.join("bad").exists());
    assert!(source.exists());
}

#[test]
fn truncated_streams_never_publish_a_complete_filename() {
    let root = tempfile::tempdir().unwrap();
    for (suffix, mut bytes) in [
        ("gz", encoded("-tgzip", b"payload")),
        ("bz2", encoded("-tbzip2", b"payload")),
        ("xz", encoded("-txz", b"payload")),
        ("zst", system_encoded("zstd", &["-q", "-c"], b"payload")),
    ] {
        bytes.truncate(bytes.len() - 4);
        let source = root.path().join(format!("truncated.{suffix}"));
        fs::write(&source, bytes).unwrap();
        let destination = root.path().join(suffix);
        assert!(extract(&source, &destination).is_err(), "{suffix}");
        assert!(!destination.join("truncated").exists());
        let partial = fs::read_dir(destination)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(partial.extension().unwrap(), "partial");
    }
}

#[test]
fn empty_and_concatenated_streams_have_a_single_output() {
    let root = tempfile::tempdir().unwrap();
    for (suffix, codec) in [("gz", "-tgzip"), ("bz2", "-tbzip2"), ("xz", "-txz")] {
        let source = root.path().join(format!("empty.{suffix}"));
        fs::write(&source, encoded(codec, b"")).unwrap();
        let destination = root.path().join(suffix);
        extract(&source, &destination).unwrap();
        assert_eq!(fs::read(destination.join("empty")).unwrap(), b"");
        let source = root.path().join(format!("joined.{suffix}"));
        fs::write(
            &source,
            [encoded(codec, b"first"), encoded(codec, b"second")].concat(),
        )
        .unwrap();
        extract(&source, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("joined")).unwrap(),
            b"firstsecond"
        );
    }
}
