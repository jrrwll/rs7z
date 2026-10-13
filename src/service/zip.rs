use crate::model::archive::ArchiveFormat;
use crate::model::archive::{ArchiveOptions, CompressionLevel};
use std::{path::Path, process::Command};

pub fn compression_level(level: CompressionLevel) -> u8 {
    match level {
        CompressionLevel::Store => 0,
        CompressionLevel::Fastest => 1,
        CompressionLevel::Standard => 5,
        CompressionLevel::Maximum => 9,
    }
}

pub fn password(options: &ArchiveOptions) -> Option<&str> {
    options
        .password
        .as_deref()
        .filter(|password| !password.is_empty())
}

pub(super) fn command_options(
    command: &mut Command,
    options: &ArchiveOptions,
    adding: bool,
    format: ArchiveFormat,
) {
    if let Some(password) = password(options) {
        command.arg(format!("-p{password}"));
        if adding && format == ArchiveFormat::Zip {
            command.arg("-mem=AES256");
        }
        if adding && format == ArchiveFormat::SevenZip {
            command.arg("-mhe=on");
        }
    }
    if adding && let Some(level) = options.compression_level {
        command.arg(format!("-mx={}", compression_level(level)));
    }
}

pub(super) fn read_command_options(
    command: &mut Command,
    options: &ArchiveOptions,
    format: ArchiveFormat,
    path: &Path,
) {
    command_options(command, options, false, format);
    // For .zip.001, 7zz must discover the Split wrapper before the inner ZIP.
    // Ordinary ZIP aliases and traditional .z01 sets still require ZIP handling.
    if format == ArchiveFormat::Zip && !super::archive::is_numbered_zip(path) {
        command.arg("-tzip");
    }
}

/// Accept a single optional 7-Zip volume size, never arbitrary command switches.
pub fn parse_volume_size(value: &str) -> Result<Option<u64>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let (number, unit) = match value.chars().last().unwrap().to_ascii_lowercase() {
        'b' => (&value[..value.len() - 1], 1u64),
        'k' => (&value[..value.len() - 1], 1024),
        'm' => (&value[..value.len() - 1], 1024u64.pow(2)),
        'g' => (&value[..value.len() - 1], 1024u64.pow(3)),
        _ => (value, 1),
    };
    number
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|size| *size > 0)
        .and_then(|size| size.checked_mul(unit))
        .map(Some)
        .ok_or_else(|| "Enter a positive volume size, such as 100M or 1G (B, K, M, G).".into())
}

pub(super) fn command_description(command: &Command) -> String {
    let args: Vec<_> = command
        .get_args()
        .map(|argument| {
            let argument = argument.to_string_lossy();
            if argument.starts_with("-p") {
                "-p[redacted]".to_owned()
            } else {
                format!("{argument:?}")
            }
        })
        .collect();
    format!("{:?} {}", command.get_program(), args.join(" "))
}

pub(super) fn require_password_if_encrypted(
    path: &Path,
    options: &ArchiveOptions,
) -> Result<(), String> {
    if matches!(
        super::archive::format(path),
        Some(ArchiveFormat::Zip | ArchiveFormat::SevenZip | ArchiveFormat::Rar)
    ) {
        let index = super::archive::read_with_options(path, options)?;
        if index.encrypted && password(options).is_none() {
            return Err("Password required for this encrypted archive.".into());
        }
        return Ok(());
    }
    Ok(())
}
