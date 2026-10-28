# 🛠️ RS7Z

[English](README.md) | [简体中文](README_zh.md)

> **The Native 7-Zip Experience for Unix-like Systems**
> *A desktop archive manager for macOS and Linux.*

**RS7Z** is a **Rust**-powered, open-source desktop archive manager inspired by [7-Zip](https://7-zip.org). It brings archive browsing, compression, extraction, testing, and checksums to macOS and Linux, using the official cross-platform `7zz` CLI and additional system tools where needed.

It combines a native graphical interface with the command-line tools familiar to terminal users.

## Runtime tools

Use the complete official [7-Zip distribution](https://7-zip.org/download.html). RS7Z resolves `7zz` in this order:

1. `RS7Z_7ZZ`, if set to an absolute executable path. An invalid override is an error.
2. `7zz` beside the RS7Z executable.
3. `PATH`.
4. `/opt/homebrew/bin` and `/usr/local/bin` on macOS.

```sh
export RS7Z_7ZZ=/absolute/path/to/7zz
7zz i
```

`7zz i` lists formats, codecs, and hashers in the installed build. RAR requires decoding support for `Rar1`, `Rar2`, `Rar3`, and `Rar5`; some Homebrew builds list the RAR format but omit those decoders. In that case, use the complete official distribution.

Creating `tar.lzma` additionally requires `xz`; creating `tar.zst` requires `zstd`. Creating POSIX TAR requires the system `tar` command with `--format=pax` support. Decoding LZMA and Zstandard uses `7zz`.

```sh
# macOS: additional compression tools
brew install xz zstd
# Debian / Ubuntu: additional compression tools
sudo apt install xz-utils zstd
```

## Format support

This table describes the current RS7Z interface. The format list printed by `7zz i` is broader than the formats exposed by RS7Z.

| Format / recognized suffixes | Create | Extract | Browse entries | Test | Edit existing archive |
| --- | --- | --- | --- | --- | --- |
| ZIP: `.zip`, `.zipx`, `.jar`, `.xpi`, `.odt`, `.ods`, `.docx`, `.xlsx`, `.epub`, `.ipa`, `.apk`, `.appx` | Yes, ZIP format | Yes | Yes | Yes | Yes |
| 7z: `.7z` | Yes | Yes | Yes | Yes | Yes |
| TAR: `.tar` | Yes | Yes | Yes | Yes | Yes, repacks |
| TAR + gzip: `.tar.gz`, `.tgz` | Yes | Yes | Yes | Yes | Yes, repacks |
| TAR + bzip2: `.tar.bz2`, `.tbz`, `.tbz2` | Yes | Yes | Yes | Yes | Yes, repacks |
| TAR + xz: `.tar.xz`, `.txz` | Yes | Yes | Yes | Yes | Yes, repacks |
| TAR + LZMA: `.tar.lzma`, `.tlz` | Yes, needs `xz` | Yes | Yes | Yes | Yes, needs `xz`, repacks |
| TAR + Zstandard: `.tar.zst`, `.tzst` | Yes, needs `zstd` | Yes | Yes | Yes | Yes, needs `zstd`, repacks |
| TAR + Unix compress: `.tar.Z`, `.taz` | No | Yes | Yes | Yes | No |
| Split ZIP: `.zip.001`; `.zip` + `.z01`, `.z02`, … | Yes, `.zip.001` style only | Yes | Yes | Yes | No |
| Split 7z: `.7z.001` | Yes | Yes | Yes | Yes | No |
| RAR: `.rar` | No | Yes, full RAR decoders required | Yes | No | No |
| Standalone streams: `.gz`/`.gzip`, `.bz2`/`.bzip2`, `.xz`, `.lzma`, `.zst`/`.zstd`, `.Z` | No | Yes, one layer to one file | No | No | No |

The creation format selector offers ZIP, 7z, TAR, tar.gz, tar.bz2, tar.xz, tar.lzma, and tar.zst. ZIP-based document/package suffixes are handled as ZIP containers; this does not generate application-specific manifests or package signatures. Standalone streams retain the source file and derive the output name by removing the final suffix.

Keep all volumes together. Open `.zip.001` or `.7z.001` for numbered sets. For traditional split ZIP sets, open the final `.zip`; selecting `.z01` resolves to that `.zip`. Split sets are read-only after creation. Other formats supported by the CLI, such as ISO, WIM, and CAB, are not currently exposed by RS7Z.

### Compression settings

| Setting | RS7Z behavior / corresponding command switch |
| --- | --- |
| Compression level | Default omits `-mx`; Store `-mx=0`, Fastest `-mx=1`, Fast `-mx=3`, Normal `-mx=5`, Maximum `-mx=7`, Ultra `-mx=9`. Plain TAR has no compression level; compressed TAR excludes Store. |
| ZIP encryption | ZipCrypt by default (`-mem=ZipCrypto`), or AES-256 (`-mem=AES256`), with a password. |
| 7z encryption | Password encryption; RS7Z also enables header encryption with `-mhe=on`. |
| 7z method | LZMA2, LZMA, PPMd, BZip2. Method-specific dictionary, word/order, and solid block controls apply outside Store mode. |
| TAR format | GNU by default via `7zz a -ttar`; POSIX via system `tar --format=pax`. Neither TAR nor its compression layers support passwords. |
| Volumes | New ZIP/7z only; B/K/M/G units, e.g. `100M`. The app passes an explicit byte count, e.g. `-v104857600b`. Empty means one archive. |

## Command-line examples

Run these examples from a folder containing `input/`; place output archives outside that source folder. Commands are manual equivalents of the archive operations, not an RS7Z command-line API. RS7Z also stages files, validates paths, and manages progress and cancellation. It adds `-sccUTF-8` and output-routing switches to its subprocesses.

`a` creates/adds, `x` extracts with paths, `l` lists, `t` tests, and `h` hashes. `-y` answers prompts affirmatively, `-spd` disables wildcard matching inside 7-Zip, and `--` ends switch parsing. Quote paths and wildcard-bearing switches in the shell. The password `example-password` below is a placeholder; explicit `-pPASSWORD` arguments can appear in process listings.

### ZIP and 7z

```sh
# ZIP; normal compression
7zz a -tzip -mx=5 -y -spd -- archive.zip input/
# ZIP with ZipCrypt (the UI label is ZipCrypt; the CLI spelling is ZipCrypto)
7zz a -tzip -mx=5 -mem=ZipCrypto -pexample-password -y -spd -- encrypted.zip input/
# ZIP with AES-256
7zz a -tzip -mx=5 -mem=AES256 -pexample-password -y -spd -- aes.zip input/

# 7z: LZMA2, 16 MiB dictionary, word size 32, 4 GiB solid block
7zz a -t7z -mx=5 -m0=LZMA2 -md=16m -mfb=32 -ms=4g -y -spd -- archive.7z input/
# Other 7z methods (use a separate output name for each example)
7zz a -t7z -m0=LZMA -md=16m -mfb=32 -y -spd -- lzma.7z input/
7zz a -t7z -m0=PPMd:mem=16m:o=6 -y -spd -- ppmd.7z input/
7zz a -t7z -m0=BZip2 -y -spd -- bzip2.7z input/
# Password and encrypted filenames/headers
7zz a -t7z -pexample-password -mhe=on -y -spd -- encrypted.7z input/

# 100 MiB volumes: archive.zip.001 / archive.7z.001, .002, ...
7zz a -tzip -v100m -y -spd -- split.zip input/
7zz a -t7z -v100m -y -spd -- split.7z input/

7zz l -slt -- archive.7z
7zz x -y -spd -ooutput -- archive.7z
7zz t -- archive.7z
7zz t -pexample-password -- encrypted.7z
7zz x -tzip -y -spd -ozip-output -- split.zip.001
7zz x -y -spd -o7z-output -- split.7z.001
7zz x -y -spd -orar-output -- archive.rar
```

For LZMA/LZMA2, dictionary size maps to `-md` and word size to `-mfb`. For PPMd, memory and model order are embedded in `-m0=PPMd:mem=…:o=…` (order 2–32). BZip2 does not use those dictionary/word controls. Solid mode uses `-ms=off`, `-ms=on`, or a size such as `-ms=4g`.

### TAR and compressed TAR

Compressed TAR uses two layers: create the TAR first, then compress that single file. Extract the outer layer and then the TAR to recover its entries. `7zz x archive.tar.gz` alone normally produces a TAR file, not its directory contents.

```sh
# GNU TAR (default)
7zz a -ttar -y -spd -- payload.tar input/
# POSIX / pax alternative; choose this instead of the preceding command
tar -c --format=pax -f payload.tar input/

# Compress the TAR with gzip, bzip2, or xz
7zz a -tgzip -mx=5 -y -spd -- payload.tar.gz payload.tar
7zz a -tbzip2 -mx=5 -y -spd -- payload.tar.bz2 payload.tar
7zz a -txz -mx=5 -y -spd -- payload.tar.xz payload.tar
# LZMA / Zstandard creation uses system encoders in RS7Z
xz --compress --stdout --format=lzma -6 -- payload.tar > payload.tar.lzma
zstd --compress --stdout --quiet --check -3 -- payload.tar > payload.tar.zst

# Decode one outer layer (choose the command for your input)
7zz x -tgzip -so -y -- payload.tar.gz > decoded.tar
7zz x -tbzip2 -so -y -- payload.tar.bz2 > decoded.tar
7zz x -txz -so -y -- payload.tar.xz > decoded.tar
7zz x -tlzma -so -y -- payload.tar.lzma > decoded.tar
7zz x -tzstd -so -y -- payload.tar.zst > decoded.tar
7zz x -tZ -so -y -- payload.tar.Z > decoded.tar

7zz l -ttar -slt -- decoded.tar
7zz x -ttar -y -spd -otar-output -- decoded.tar
7zz t -ttar -- decoded.tar
```

RS7Z uses a temporary workspace for these layers. Testing compressed TAR decodes the outer layer first (checking its integrity where the codec supports it), then reads and tests the inner TAR. TAR itself has no per-file content checksum comparable to ZIP CRC32, so a successful TAR test is not a cryptographic integrity guarantee.

### Standalone compressed streams

```sh
# report.txt.gz -> report.txt; stdout contains the decoded file bytes
7zz x -tgzip -so -y -spd -- report.txt.gz > report.txt
```

Replace `-tgzip` with `-tbzip2`, `-txz`, `-tlzma`, `-tzstd`, or `-tZ` for the other supported streams. RS7Z refuses to replace an existing output file; ordinary shell `>` redirection does not provide that protection.

## CRC and checksums

Right-click a regular filesystem file or an entry inside a browsable archive and choose **CRC**. Ordinary filesystem directories do not expose this menu; archive directories do. Results use two left-aligned columns and support copying individual values and **Copy all**.

| Menu / result algorithm | 7zz command |
| --- | --- |
| CRC-32 | `7zz h -scrcCRC32 -spd -- file.bin` |
| CRC-64 | `7zz h -scrcCRC64 -spd -- file.bin` |
| XXH64 | `7zz h -scrcXXH64 -spd -- file.bin` |
| SHA-1 | `7zz h -scrcSHA1 -spd -- file.bin` |
| SHA-256 | `7zz h -scrcSHA256 -spd -- file.bin` |
| BLAKE2sp | `7zz h -scrcBLAKE2SP -spd -- file.bin` |
| MD5 | `7zz h -scrcMD5 -spd -- file.bin` |
| `*` — all 10 | `7zz h '-scrc*' -spd -- file.bin` |

The ten results for `*` are **CRC32, CRC64, SHA256, SHA1, BLAKE2sp, MD5, XXH64, SHA384, SHA512, and SHA3-256**. SHA384, SHA512, and SHA3-256 appear through `*`; there are no separate RS7Z menu entries for them. A compatible `7zz` build must provide all ten hashers. The CLI also accepts `-scrcSHA384`, `-scrcSHA512`, and `-scrcSHA3-256` individually.

```sh
# Dotfiles work like any other file; use their explicit paths
7zz h '-scrc*' -spd -- "$HOME/.zshrc"
# Multiple files: the UI reads the aggregate "for data:" summaries
7zz h -scrcSHA256 -spd -- first.bin second.bin
# CLI-only directory example (ordinary directories have no CRC menu in RS7Z)
7zz h '-scrc*' -spd -- input/

# Hash an archive entry's decoded content, not the archive container
7zz x -y -spd -ochecksum-stage -- archive.7z docs/file.txt
7zz h '-scrc*' -spd -- checksum-stage/docs/file.txt
```

RS7Z extracts selected archive entries into temporary directories before hashing them and uses the archive password when needed. Hashing `archive.7z` as a filesystem file instead checks the compressed container bytes. For multiple files or directories, 7zz's `for data:` result is an aggregate, not a list of individual file digests; do not interpret it as the digest of a concatenated stream. The list's CRC32 column is stored archive metadata and may show `--`; the CRC menu calculates fresh checksums from file contents. CRC/XXH64 are useful for accidental corruption checks; MD5/SHA-1 should not be used for security-sensitive verification.

## Builtin Themes

| Theme file | Appearance |
| --- | --- |
| `themes/apple.toml` | Native system font, light gray chrome, blue selection |
| `themes/opencode.ai.toml` | Warm white, ink selection, monospaced UI |
| `themes/claude.toml` | Cream surfaces, coral selection, warm text |
| `themes/nvidia.toml` | White and black structure, NVIDIA green selection |
| `themes/tesla.toml` | White and carbon gray, electric-blue selection |
| `themes/x.ai.toml` | Near-black surfaces, white selection (dark mode) |
| `themes/binance.toml` | Dark trading surfaces, yellow selection with ink text |
| `themes/coinbase.toml` | White surfaces, Coinbase blue selection, rounded controls |
| `themes/playstation.toml` | Light gray chrome, PlayStation blue selection |
| `themes/pinterest.toml` | Cream chrome, red selection, soft rounded surfaces |
| `themes/notion.toml` | Warm gray surfaces, purple selection, lavender hover |
| `themes/starbucks.toml` | Cream and ceramic surfaces, green selection |
| `themes/zapier.toml` | Warm cream surfaces, orange selection with coffee text |
