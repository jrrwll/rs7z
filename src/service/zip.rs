use crate::model::archive::ArchiveFormat;
use crate::model::archive::{
    ArchiveOptions, CompressionLevel, SevenZipMethod, SolidBlockSize, ZipEncryption,
};
use crate::model::operations::OperationError;
use std::{path::Path, process::Command};

pub fn compression_level(level: CompressionLevel) -> u8 {
    match level {
        CompressionLevel::Store => 0,
        CompressionLevel::Fastest => 1,
        CompressionLevel::Fast => 3,
        CompressionLevel::Normal => 5,
        CompressionLevel::Maximum => 7,
        CompressionLevel::Ultra => 9,
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
            command.arg(match options.zip_encryption {
                ZipEncryption::ZipCrypt => "-mem=ZipCrypto",
                ZipEncryption::Aes256 => "-mem=AES256",
            });
        }
        if adding && format == ArchiveFormat::SevenZip {
            command.arg("-mhe=on");
        }
    }
    if adding && let Some(level) = options.compression_level {
        command.arg(format!("-mx={}", compression_level(level)));
    }
}

pub(super) fn seven_zip_command_options(
    command: &mut Command,
    options: &ArchiveOptions,
) -> Result<(), OperationError> {
    if options.compression_level == Some(CompressionLevel::Store) {
        return Ok(());
    }
    let settings = options.seven_zip;
    if settings == Default::default() {
        return Ok(());
    }
    let method = settings.method.unwrap_or(SevenZipMethod::Lzma2);
    if let Some(size) = settings.dictionary_size
        && !(64 * 1024..=3840 * 1024 * 1024).contains(&size)
    {
        return Err(OperationError::Failed("Invalid 7z dictionary size.".into()));
    }
    match method {
        SevenZipMethod::Lzma2 | SevenZipMethod::Lzma => {
            command.arg(match method {
                SevenZipMethod::Lzma2 => "-m0=LZMA2",
                _ => "-m0=LZMA",
            });
            if let Some(size) = settings.dictionary_size {
                command.arg(format!("-md={size}b"));
            }
            if let Some(size) = settings.word_size {
                if !(5..=273).contains(&size) {
                    return Err(OperationError::Failed("Invalid LZMA word size.".into()));
                }
                command.arg(format!("-mfb={size}"));
            }
        }
        SevenZipMethod::Ppmd => {
            if settings
                .word_size
                .is_some_and(|size| !(2..=32).contains(&size))
            {
                return Err(OperationError::Failed(
                    "PPMd word size must be 2 to 32.".into(),
                ));
            }
            let mut method = "-m0=PPMd".to_owned();
            if let Some(size) = settings.dictionary_size {
                method.push_str(&format!(":mem={size}b"));
            }
            if let Some(size) = settings.word_size {
                method.push_str(&format!(":o={size}"));
            }
            command.arg(method);
        }
        SevenZipMethod::Bzip2 => {
            if settings.dictionary_size.is_some() || settings.word_size.is_some() {
                return Err(OperationError::Failed(
                    "BZip2 does not support the selected dictionary or word size.".into(),
                ));
            }
            command.arg("-m0=BZip2");
        }
    }
    if let Some(size) = settings.solid_block_size {
        match size {
            SolidBlockSize::NonSolid => command.arg("-ms=off"),
            SolidBlockSize::Solid => command.arg("-ms=on"),
            SolidBlockSize::Size(size)
                if (1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&size) =>
            {
                command.arg(format!("-ms={size}b"))
            }
            SolidBlockSize::Size(_) => {
                return Err(OperationError::Failed(
                    "Invalid 7z solid block size.".into(),
                ));
            }
        };
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_levels_match_7_zip() {
        for (level, expected) in [
            (CompressionLevel::Store, 0),
            (CompressionLevel::Fastest, 1),
            (CompressionLevel::Fast, 3),
            (CompressionLevel::Normal, 5),
            (CompressionLevel::Maximum, 7),
            (CompressionLevel::Ultra, 9),
        ] {
            assert_eq!(compression_level(level), expected);
        }
    }
}
