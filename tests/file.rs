use rs7z::service::file::{
    format_info_size, format_modified, format_modified_seconds, format_size, read_directory,
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn size_labels() {
    assert_eq!(format_size(100), "100 bytes");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_info_size(100), "100 bytes");
    assert_eq!(format_info_size(10240), "10240 (10 KB)");
    assert_eq!(format_info_size(1536), "1536 (1.5 KB)");
}

#[test]
fn file_modified_time_is_available_in_directory_listing() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let folder =
        std::env::temp_dir().join(format!("rs7z-file-test-{}-{nonce}", std::process::id()));
    fs::create_dir(&folder).unwrap();
    let path = folder.join("modified.txt");
    fs::write(&path, b"content").unwrap();

    let item = read_directory(&folder)
        .unwrap()
        .into_iter()
        .find(|item| item.path == path)
        .unwrap();
    let modified = item.modified.unwrap();
    assert_eq!(modified, fs::metadata(&path).unwrap().modified().unwrap());
    assert_eq!(item.created, fs::metadata(&path).unwrap().created().ok());
    assert_eq!(item.accessed, fs::metadata(&path).unwrap().accessed().ok());
    assert!(item.archive.is_none());
    assert_eq!(format_modified(None), "--");
    assert_eq!(format_modified_seconds(None), "--");
    assert_eq!(format_modified(Some(SystemTime::UNIX_EPOCH)).len(), 16);
    let with_seconds = format_modified_seconds(Some(SystemTime::UNIX_EPOCH));
    assert_eq!(with_seconds.len(), 19);
    assert!(with_seconds.ends_with(":00"));

    fs::remove_dir_all(folder).unwrap();
}
