# rs7z

A desktop archive manager built with Rust and GPUI Kit.

Launch with an optional directory or browsable archive path:

```sh
rs7z
rs7z .
rs7z /path/to/folder
rs7z "./archives/backup files.zip"
# From the source checkout
cargo run -- ./backup.7z
```

Without a path, rs7z opens the user's home directory. Use `rs7z .` to open the
current working directory. Relative paths resolve against the working directory.
Browsable archives open through the normal browser,
including password prompts for encrypted archives. RAR and standalone streams
are not accepted as startup paths. Missing paths, unsupported files, and extra
arguments produce an English terminal error and exit with status 2.

## Archive CLI dependency

Install the **official 7-Zip CLI (`7zz`)**. The migration is tested with 7-Zip
26.04 on macOS arm64. **RAR requires the complete official distribution** from
[7-zip.org](https://7-zip.org/download.html). Some package-manager builds omit
RAR decoders: the Homebrew 26.04 build can list RAR headers but cannot decode
compressed RAR contents. rs7z checks the decoder list and reports this explicitly.

After unpacking the official distribution:

```sh
/absolute/path/to/7zz i
RS7Z_7ZZ=/absolute/path/to/7zz cargo run
```

rs7z resolves the CLI in this order:

1. `RS7Z_7ZZ`, if set, must be an absolute path to an executable file. An invalid
   override is an error; it does not silently fall back to another installation.
2. `7zz` beside the rs7z executable (for example, `App.app/Contents/MacOS/7zz`).
3. `7zz` in `PATH`.
4. On macOS, `/opt/homebrew/bin/7zz` and `/usr/local/bin/7zz`, so Finder launches
   can locate Homebrew even without a shell-configured `PATH`.

To select an installation explicitly:

```sh
RS7Z_7ZZ=/absolute/path/to/7zz cargo run
```

The path is passed directly to the operating system, so spaces in it are supported;
do not put shell arguments in `RS7Z_7ZZ`. On Windows the searched filename is
`7zz.exe`; use `RS7Z_7ZZ` for a differently named official executable.
Discovered paths are resolved before a command changes its working directory.
All archive CLI commands request UTF-8 console output.

There is no automatic fallback to `7z`, `7za`, or p7zip. p7zip-specific codecs
inside `.7z` files, such as ZSTD and LZ4, are outside the supported format set.
This does not remove ZSTD **ZIP/ZIPX** decoding or `.tar.zst` support.

New ZIP and 7z archives support an optional volume size (for example, `100M`).
ZIP volumes created by `7zz` are named `archive.zip.001`, `.002`, and so on;
these are byte-split ZIP files, not traditional `.z01` / `.zip` sets. Choose a
`.zip` name when creating ZIP volumes. Keep every volume together and open the
`.zip.001` or `.7z.001` file to browse, test, extract, or copy entries out.
Existing split archives are readonly. Traditional `.z01` / `.zip` sets remain
supported for reading, but are not created by rs7z. Existing volume files block
creation with the same archive name; cancellation preserves partial volumes.

RAR listing, password verification, and extraction use `7zz`, without an embedded
UnRAR dependency. Extraction displays live CLI output in a selectable, read-only
log instead of a progress bar. The log remains open after success, failure, or
cancellation; only its recent output is retained to bound memory use. Cancellation
stops the process and preserves partial files. CRC failures remove the files
identified as damaged while preserving successfully extracted files; 7zz may
continue to later entries after an error. RAR remains extraction-only.

One password is supplied for the entire archive. Archives requiring different
passwords for different entries are unsupported; there are no interactive
password or volume prompts. Place all RAR volumes together and select the first.

Standalone gzip (`.gz`, `.gzip`), bzip2 (`.bz2`, `.bzip2`), XZ (`.xz`),
Zstandard (`.zst`, `.zstd`), LZMA (`.lzma`), and Unix compress (`.Z`)
support extraction through `7zz`.
Suffixes are case-insensitive. Double-click opens the extraction dialog; these
files cannot be browsed, tested, created, or edited as archives.

Extraction decodes exactly one layer. The output name strips only the final
suffix (`report.txt.gz` becomes `report.txt`), ignoring embedded names. The
default destination is the source's folder, with no additional subdirectory.
The source is kept, existing files are never overwritten, and failures or
cancellation retain a randomly named `.partial` file with its path displayed
in a selectable message. Concatenated gzip/bzip2/XZ streams yield one output.
Formats without checksums cannot provide the same integrity guarantees as gzip
or XZ.

Known compressed TAR suffixes and aliases take precedence and remain browsable:
`.tar.gz` / `.tgz`, `.tar.bz2` / `.tbz` / `.tbz2`, `.tar.xz` / `.txz`,
`.tar.lzma` / `.tlz`, `.tar.zst` / `.tzst`, and `.tar.Z` / `.taz`.
An ordinary `.gz` containing TAR bytes produces a TAR file without unpacking
its entries. LZMA86 (`.lzma86`), Base64 (`.b64`, `.base64`), other
`.tar.<codec>` combinations, and plain `.tar` files are unsupported. `.tar.Z` / `.taz` remain
readonly; creation and editing are not offered.

Compressed TAR streams use system `xz` for `.tar.lzma` / `.tlz` and system
`zstd` for `.tar.zst` / `.tzst` compression. Decompression uses `7zz`, so browsing,
extraction, testing, and copying entries out do not require `xz` or `zstd`.
Creation and editing remain supported. rs7z checks for the relevant executable before
compression; a missing tool produces an English, selectable error with installation
instructions and a **Copy all** button. On macOS:

```sh
brew install xz zstd
```

On Linux, rs7z uses `/etc/os-release` to suggest the appropriate command:

```sh
# Debian / Ubuntu
sudo apt install xz-utils zstd
# Fedora / RHEL
sudo dnf install xz zstd
# Arch Linux
sudo pacman -S xz zstd
```

Executables are resolved from `PATH`, with `/opt/homebrew/bin` and
`/usr/local/bin` as macOS fallbacks for Finder launches. Restart rs7z after changing
`PATH`. Windows users can install both with `scoop install xz zstd` if Scoop is
available. Installation is performed by the user, not by rs7z.

System compression processes can be cancelled; partial staging results are retained.
Their progress reports phase start and completion rather than a byte percentage.
ZIP browsing, password verification, extraction, and editing use `7zz`.
Edits run on a private copy and replace the original only after success. Delete,
rename, and move operations preserve unchanged compressed entries; copying within
a ZIP extracts and recompresses only the selected entries. Failed or cancelled
operations retain their workspace. Password verification tests the whole archive,
and existing encryption follows `7zz` behavior (ZipCrypto is not upgraded).

The `zip` crate is used only in tests, with Deflate/AES enabled, to create independent
fixtures and check results. It is not part of the application build. Neither
`lzma-rust2` nor `zstd` remains in the Rust dependency tree.
Archive edits that
already use extraction and repacking still use that workflow.

The repository does not bundle or download a CLI binary. Distributors choosing
to bundle one must provide a build for their target platform, include its
upstream distribution notices, and integrate it into their signing/package process.

## macOS DMG packaging

`cargo packager` automatically discovers `packager.toml` from the repository root.
The shared configuration produces DMG on macOS and AppImage on Linux; unsupported
formats are skipped. All packages are written to `target/packages/`.

On macOS, install Xcode Command Line Tools and cargo-packager, then run from the
repository root:

```sh
cargo install cargo-packager --version 0.11.8 --locked
cargo packager
```

The configuration builds the release binary for the host architecture and creates
`target/packages/rs7z_0.1.0_aarch64.dmg` on Apple Silicon
(`rs7z_0.1.0_x64.dmg` on Intel). The app uses `assets/rs7z.icns`; the DMG includes an
Applications shortcut for drag-to-install. The deployment target is macOS 11.0.
Keep `version` in `packager.toml` in sync with `Cargo.toml` when releasing.
To retain a standalone `target/packages/rs7z.app` alongside the DMG, run
`cargo packager --formats app,dmg`. To build only the app, use `--formats app`.

The package uses the existing system CLI discovery for `7zz`, `xz`, and `zstd`;
these executables are not included. The first DMG build downloads cargo-packager's
`create-dmg` helper and may request permission to control Finder for the layout.
For CI without Finder layout automation:

```sh
CI=true cargo packager
```

The default configuration has no Developer ID signature or notarization. For
public distribution, configure `macos.signingIdentity` with your Developer ID
Application certificate and provide notarization credentials through
`APPLE_KEYCHAIN_PROFILE` or cargo-packager's other `APPLE_*` environment variables.

## Linux AppImage packaging

Build natively on Linux with Rust and the native GPUI build dependencies for your
distribution installed. Use cargo-packager, as for macOS, from the repository root:

```sh
cargo install cargo-packager --version 0.11.8 --locked
cargo packager
```

The configuration builds the release binary for the host architecture and creates
`target/packages/rs7z_0.1.0_x86_64.AppImage` on x86-64 Linux
(`rs7z_0.1.0_aarch64.AppImage` on ARM64). It uses the PNG icons in `assets/icons/`
and generates a desktop entry. cargo-packager uses linuxdeploy to bundle linked
libraries; the first build downloads its AppImage helpers and requires network
access. Build on the oldest Linux distribution you intend to support, since the
result retains the build system's glibc requirements. AppImage packaging must run
on Linux, using the same `packager.toml` as macOS.

The archive tools `7zz`, `xz`, and `zstd` are installed separately on the user's
system, following the CLI dependency instructions above. Run the package with:

```sh
chmod +x target/packages/rs7z_0.1.0_x86_64.AppImage
./target/packages/rs7z_0.1.0_x86_64.AppImage
# Open the current directory instead of the user's home directory
./target/packages/rs7z_0.1.0_x86_64.AppImage .
```

If FUSE is unavailable on the target system, use AppImage's extraction mode:

```sh
./target/packages/rs7z_0.1.0_x86_64.AppImage --appimage-extract-and-run
```

## Manual GitHub pre-releases

In GitHub Actions, select **Build pre-release** and choose **Run workflow** on the
branch to package. The workflow reads the version from `Cargo.toml`, updates the
packager configuration for the build, and packages natively on macOS (Apple
Silicon and Intel) and Ubuntu 24.04 (AMD64 and ARM64). Once all four builds pass,
it publishes a GitHub pre-release tagged `v<version>-pre.<run-number>` at the
selected commit. Re-running the same workflow run replaces that run's assets.

For version `0.1.0`, the release contains:

```text
rs7z_0.1.0_aarch64.dmg
rs7z_0.1.0_x64.dmg
rs7z_0.1.0_amd64.AppImage
rs7z_0.1.0_amd64.AppImage.sha256
rs7z_0.1.0_aarch64.AppImage
rs7z_0.1.0_aarch64.AppImage.sha256
```

The `.sha256` files are SHA-256 checksums, not digital signatures. Download one
alongside its AppImage and run `sha256sum --check <filename>.AppImage.sha256`.
DMGs have no separate checksum file. The Linux packages retain Ubuntu 24.04's
glibc requirements; archive tools are installed separately as described above.
No extra GitHub secret is required: the publication job uses `GITHUB_TOKEN`
with `contents: write`.

## Development checks

```sh
cargo fmt --check
cargo check --all-targets --locked
cargo test --lib --tests --no-fail-fast --locked
cargo clippy --all-targets --locked
```

Archive tests require `xz`, `zstd`, and a complete `7zz` with RAR decoders
(set `RS7Z_7ZZ` as above);
missing dependencies fail instead of silently skipping
coverage. Existing split-ZIP tests also use the `zip` CLI. ZIPX decoder samples
are checked into `tests/fixtures` so testing does not depend on CLI encoders that
the official 7-Zip build does not provide. See `tests/fixtures/README.md` for provenance.
