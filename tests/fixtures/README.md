# Archive test fixtures

`sample.tar.Z` is generated locally from small test files.

## ZIPX codec samples

`zipx-deflate64.zipx`, `zipx-bzip2.zipx`, `zipx-lzma.zipx`,
`zipx-ppmd.zipx`, and `zipx-zstd.zipx` were generated locally with
p7zip 17.05 (Homebrew package 17.06) during the migration to official 7-Zip.
Each contains only `item.txt`: the UTF-8 bytes `ZIPX codec interoperability\n`
repeated 100 times, with its source modification time set to Unix timestamp
946684800. The source payload is synthetic test data created for this project.

Generation command, with `METHOD` set to `Deflate64`, `BZip2`, `LZMA`, `PPMd`,
or `ZSTD`:

```sh
7z a -tzip -mm=METHOD sample.zipx item.txt
```

The files are fixed interoperability inputs; p7zip is not required to run tests.
Official 7zz 26.04 can decode these samples but cannot encode ZSTD ZIP.
Tests cover CLI validation/extraction and native entry export/rename followed
by validation of the edited archive.

## RAR samples

RAR fixtures are decoded without modification from the uuencoded libarchive test corpus:

- `sample-rar4.rar`: `test_read_format_rar.rar.uu`
- `sample-rar5.rar`: `test_read_format_rar5_stored.rar.uu`
- `rar4-encrypted.rar`: `test_read_format_rar4_encrypted.rar.uu`
- `rar5-encrypted.rar`: `test_read_format_rar5_encrypted.rar.uu`
- `rar4-encrypted-headers.rar`: `test_read_format_rar4_encrypted_filenames.rar.uu`
- `rar5-encrypted-headers.rar`: `test_read_format_rar5_encrypted_filenames.rar.uu`
- `rar5-solid.rar`: `test_read_format_rar5_multiple_files_solid.rar.uu`
- `rar4-volume.part1.rar`, `rar4-volume.part2.rar`, `rar4-volume.part3.rar`:
  `test_rar_multivolume_single_file.part1.rar.uu` through `part3.rar.uu`

Source: https://github.com/libarchive/libarchive/tree/master/libarchive/test

Passwords and expected contents: `test_read_format_rar_encryption.c`. Header-encrypted archives use `password`; mixed-entry archives use `password` for b.txt and `password2` for d.txt.
The mixed-entry fixtures are retained for provenance; rs7z supports one password
per archive and does not support extracting those archives with multiple passwords.

Upstream license notice:

```text
/*
 * Copyright (c) 2003-2018
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer
 *    in this position and unchanged.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
```
