# 🛠️ RS7Z

[English](README.md) | [简体中文](README_zh.md)

> **面向 Unix 类系统的原生 7-Zip 使用体验**
> *适用于 macOS 和 Linux 的桌面压缩文件管理器。*

**RS7Z** 是一个使用 **Rust** 开发、受 [7-Zip](https://7-zip.org) 启发的开源桌面压缩文件管理器。它通过官方跨平台 `7zz` 命令行工具及必要的系统工具，在 macOS 和 Linux 上提供压缩包浏览、压缩、解压、测试和校验和计算功能。

它将原生图形界面与终端用户熟悉的命令行工具结合起来。

## 运行依赖

请使用完整的官方 [7-Zip 发行包](https://7-zip.org/download.html)。RS7Z 按以下顺序查找 `7zz`：

1. 环境变量 `RS7Z_7ZZ` 指定的可执行文件绝对路径。路径无效时会报错。
2. RS7Z 可执行文件所在目录中的 `7zz`。
3. `PATH` 中的 `7zz`。
4. macOS 上的 `/opt/homebrew/bin` 和 `/usr/local/bin`。

```sh
export RS7Z_7ZZ=/absolute/path/to/7zz
7zz i
```

`7zz i` 会列出当前构建的格式、编解码器和哈希算法。RAR 需要 `Rar1`、`Rar2`、`Rar3`、`Rar5` 解码支持；部分 Homebrew 构建虽然列出 RAR 格式，但没有这些解码器，此时应改用完整的官方发行包。

创建 `tar.lzma` 还需要 `xz`；创建 `tar.zst` 需要 `zstd`。创建 POSIX TAR 需要系统 `tar` 命令支持 `--format=pax`。LZMA 和 Zstandard 的解码由 `7zz` 完成。

```sh
# macOS：额外的压缩工具
brew install xz zstd
# Debian / Ubuntu：额外的压缩工具
sudo apt install xz-utils zstd
```

## 格式支持情况

下表描述 RS7Z 当前界面的支持范围。`7zz i` 列出的格式比 RS7Z 实际开放的格式更多。

| 格式 / 可识别后缀 | 创建压缩包 | 解压 | 浏览内部条目 | 测试 | 修改已有压缩包 |
| --- | --- | --- | --- | --- | --- |
| ZIP：`.zip`、`.zipx`、`.jar`、`.xpi`、`.odt`、`.ods`、`.docx`、`.xlsx`、`.epub`、`.ipa`、`.apk`、`.appx` | 支持，以 ZIP 格式创建 | 支持 | 支持 | 支持 | 支持 |
| 7z：`.7z` | 支持 | 支持 | 支持 | 支持 | 支持 |
| TAR：`.tar` | 支持 | 支持 | 支持 | 支持 | 支持，重新打包 |
| TAR + gzip：`.tar.gz`、`.tgz` | 支持 | 支持 | 支持 | 支持 | 支持，重新打包 |
| TAR + bzip2：`.tar.bz2`、`.tbz`、`.tbz2` | 支持 | 支持 | 支持 | 支持 | 支持，重新打包 |
| TAR + xz：`.tar.xz`、`.txz` | 支持 | 支持 | 支持 | 支持 | 支持，重新打包 |
| TAR + LZMA：`.tar.lzma`、`.tlz` | 支持，需要 `xz` | 支持 | 支持 | 支持 | 支持，需要 `xz`，重新打包 |
| TAR + Zstandard：`.tar.zst`、`.tzst` | 支持，需要 `zstd` | 支持 | 支持 | 支持 | 支持，需要 `zstd`，重新打包 |
| TAR + Unix compress：`.tar.Z`、`.taz` | 不支持 | 支持 | 支持 | 支持 | 不支持 |
| 分卷 ZIP：`.zip.001`；`.zip` + `.z01`、`.z02` 等 | 支持，仅创建 `.zip.001` 形式 | 支持 | 支持 | 支持 | 不支持 |
| 分卷 7z：`.7z.001` | 支持 | 支持 | 支持 | 支持 | 不支持 |
| RAR：`.rar` | 不支持 | 支持，需要完整 RAR 解码器 | 支持 | 不支持 | 不支持 |
| 独立压缩流：`.gz`/`.gzip`、`.bz2`/`.bzip2`、`.xz`、`.lzma`、`.zst`/`.zstd`、`.Z` | 不支持 | 支持，解码一层得到单个文件 | 不支持 | 不支持 | 不支持 |

创建弹窗的格式选择器提供 ZIP、7z、TAR、tar.gz、tar.bz2、tar.xz、tar.lzma、tar.zst。基于 ZIP 的文档和安装包后缀按 ZIP 容器处理，不会生成特定应用所需的清单或安装包签名。独立压缩流解压时保留源文件，并通过去掉最后一个后缀确定输出文件名。

所有分卷应保存在同一目录。数字分卷通过 `.zip.001` 或 `.7z.001` 打开；传统分卷 ZIP 通过末尾的 `.zip` 打开，选择 `.z01` 时会自动定位对应 `.zip`。分卷创建后为只读。CLI 支持的 ISO、WIM、CAB 等其他格式，目前尚未在 RS7Z 中开放。

### 压缩参数

| 设置 | RS7Z 行为 / 对应命令参数 |
| --- | --- |
| 压缩等级 | 默认不传 `-mx`；仅存储 `-mx=0`、极速压缩 `-mx=1`、快速压缩 `-mx=3`、标准压缩 `-mx=5`、最大压缩 `-mx=7`、极限压缩 `-mx=9`。普通 TAR 没有压缩等级；带压缩层的 TAR 不提供“仅存储”。 |
| ZIP 加密 | 默认 ZipCrypt（`-mem=ZipCrypto`），可选 AES-256（`-mem=AES256`），设置密码后生效。 |
| 7z 加密 | 支持密码加密；RS7Z 同时通过 `-mhe=on` 启用文件头加密。 |
| 7z 压缩方法 | LZMA2、LZMA、PPMd、BZip2。非“仅存储”模式下，按压缩方法提供字典大小、单词大小/阶数和固实数据大小设置。 |
| TAR 格式 | 默认 GNU，通过 `7zz a -ttar` 创建；POSIX 通过系统 `tar --format=pax` 创建。TAR 及其压缩层均不支持密码。 |
| 分卷大小 | 仅新建 ZIP/7z 支持；可输入 B/K/M/G 单位，例如 `100M`。应用会传入明确的字节数，例如 `-v104857600b`。留空表示不分卷。 |

## 命令行示例

以下示例在包含 `input/` 的目录执行，输出压缩包应放在源目录之外。这些命令是压缩操作的手动等价示例，不是 RS7Z 的命令行接口。RS7Z 还会暂存文件、校验路径，以及管理进度和取消操作；调用子进程时会附加 `-sccUTF-8` 和输出流控制参数。

`a` 表示创建/添加，`x` 表示保留路径解压，`l` 表示列表，`t` 表示测试，`h` 表示计算哈希。`-y` 表示自动确认，`-spd` 禁用 7-Zip 内部的通配符匹配，`--` 表示结束命令参数解析。路径以及包含通配符的参数应在 shell 中加引号。下文的 `example-password` 仅为示例密码；显式传入的 `-pPASSWORD` 可能出现在进程列表中。

### ZIP 和 7z

```sh
# ZIP，标准压缩
7zz a -tzip -mx=5 -y -spd -- archive.zip input/
# ZIP，ZipCrypt 加密（界面名称为 ZipCrypt，CLI 参数拼写为 ZipCrypto）
7zz a -tzip -mx=5 -mem=ZipCrypto -pexample-password -y -spd -- encrypted.zip input/
# ZIP，AES-256 加密
7zz a -tzip -mx=5 -mem=AES256 -pexample-password -y -spd -- aes.zip input/

# 7z：LZMA2，16 MiB 字典，单词大小 32，4 GiB 固实数据块
7zz a -t7z -mx=5 -m0=LZMA2 -md=16m -mfb=32 -ms=4g -y -spd -- archive.7z input/
# 其他 7z 压缩方法，每个示例使用不同输出文件名
7zz a -t7z -m0=LZMA -md=16m -mfb=32 -y -spd -- lzma.7z input/
7zz a -t7z -m0=PPMd:mem=16m:o=6 -y -spd -- ppmd.7z input/
7zz a -t7z -m0=BZip2 -y -spd -- bzip2.7z input/
# 设置密码并加密文件名/文件头
7zz a -t7z -pexample-password -mhe=on -y -spd -- encrypted.7z input/

# 100 MiB 分卷：archive.zip.001 / archive.7z.001、.002 等
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

LZMA/LZMA2 的字典大小对应 `-md`，单词大小对应 `-mfb`。PPMd 的内存和模型阶数写在 `-m0=PPMd:mem=…:o=…` 中，阶数范围为 2–32。BZip2 不使用这些字典/单词大小控件。固实模式对应 `-ms=off`、`-ms=on` 或 `-ms=4g` 等大小参数。

### TAR 和带压缩层的 TAR

带压缩层的 TAR 分为两层：先打包为 TAR，再压缩这个单文件。解压时先去掉外层压缩，再解开 TAR 才能得到内部文件。单独执行 `7zz x archive.tar.gz` 通常只得到 TAR 文件，不会直接得到内部目录内容。

```sh
# GNU TAR（默认）
7zz a -ttar -y -spd -- payload.tar input/
# POSIX / pax 替代方案，与上一条创建命令二选一
tar -c --format=pax -f payload.tar input/

# 使用 gzip、bzip2 或 xz 压缩 TAR
7zz a -tgzip -mx=5 -y -spd -- payload.tar.gz payload.tar
7zz a -tbzip2 -mx=5 -y -spd -- payload.tar.bz2 payload.tar
7zz a -txz -mx=5 -y -spd -- payload.tar.xz payload.tar
# RS7Z 创建 LZMA / Zstandard 时使用系统编码器
xz --compress --stdout --format=lzma -6 -- payload.tar > payload.tar.lzma
zstd --compress --stdout --quiet --check -3 -- payload.tar > payload.tar.zst

# 解码一层外部压缩，按输入格式选择一条命令
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

RS7Z 使用临时目录处理这些中间层。测试带压缩层的 TAR 时，先解码外层并校验编解码器所支持的完整性信息，再读取和测试内部 TAR。TAR 本身没有类似 ZIP CRC32 的逐文件内容校验和，因此测试成功不代表具备密码学意义上的完整性保证。

### 独立压缩流

```sh
# report.txt.gz -> report.txt，标准输出为解码后的文件字节
7zz x -tgzip -so -y -spd -- report.txt.gz > report.txt
```

其他压缩流将 `-tgzip` 替换为 `-tbzip2`、`-txz`、`-tlzma`、`-tzstd` 或 `-tZ`。RS7Z 会拒绝覆盖已有输出文件；普通 shell 的 `>` 重定向不提供这一保护。

## CRC 与校验和

右键普通文件或可浏览压缩包中的条目，选择 **CRC**。普通文件系统目录不提供此菜单，压缩包内的目录支持。结果采用左对齐的两列展示，支持复制单个值和“全部复制”。

| 菜单 / 结果算法 | 7zz 命令 |
| --- | --- |
| CRC-32 | `7zz h -scrcCRC32 -spd -- file.bin` |
| CRC-64 | `7zz h -scrcCRC64 -spd -- file.bin` |
| XXH64 | `7zz h -scrcXXH64 -spd -- file.bin` |
| SHA-1 | `7zz h -scrcSHA1 -spd -- file.bin` |
| SHA-256 | `7zz h -scrcSHA256 -spd -- file.bin` |
| BLAKE2sp | `7zz h -scrcBLAKE2SP -spd -- file.bin` |
| MD5 | `7zz h -scrcMD5 -spd -- file.bin` |
| `*`：全部 10 种 | `7zz h '-scrc*' -spd -- file.bin` |

`*` 展示 **CRC32、CRC64、SHA256、SHA1、BLAKE2sp、MD5、XXH64、SHA384、SHA512、SHA3-256** 共十种结果。SHA384、SHA512、SHA3-256 通过 `*` 展示，RS7Z 没有为它们单独提供菜单项。使用的 `7zz` 构建需要具备全部十种算法。CLI 也允许分别使用 `-scrcSHA384`、`-scrcSHA512`、`-scrcSHA3-256`。

```sh
# 点开头的隐藏文件与普通文件相同，直接指定完整路径
7zz h '-scrc*' -spd -- "$HOME/.zshrc"
# 多个文件：界面读取汇总的 "for data:" 结果
7zz h -scrcSHA256 -spd -- first.bin second.bin
# 仅 CLI 示例：RS7Z 不为普通目录提供 CRC 菜单
7zz h '-scrc*' -spd -- input/

# 校验压缩包条目解码后的内容，而不是压缩包容器
7zz x -y -spd -ochecksum-stage -- archive.7z docs/file.txt
7zz h '-scrc*' -spd -- checksum-stage/docs/file.txt
```

RS7Z 会先将选中的压缩包条目解压到临时目录，再计算校验和；需要时使用压缩包密码。若在普通文件目录中对 `archive.7z` 计算校验和，校验的则是压缩容器自身的字节。对于多个文件或目录，7zz 的 `for data:` 是汇总结果，不是逐文件摘要列表，也不能理解为文件拼接后的摘要。列表中的 CRC32 列来自压缩包保存的元数据，可能显示 `--`；CRC 菜单则从文件内容重新计算。CRC/XXH64 适用于检查意外损坏；MD5/SHA-1 不应用于安全敏感的验证。

## 语言设置

打开“设置”（齿轮图标），在“常规”选项卡中选择英语、简体中文或外部语言文件。点击“应用”后重启 rs7z 生效。内置翻译分别保存在 `locales/en.toml` 和 `locales/zh-CN.toml`。英文文件的值留空，翻译缺失或为空时使用键本身的英文文本。

外部语言文件采用相同的扁平 TOML 格式，以英文文本为键，因此可以只提供部分翻译：

```toml
"Settings" = "Paramètres"
"Copy all" = "Tout copier"
"%{count} items" = "%{count} éléments"
```

缺失或无效的单个条目，包括空值或 `%{placeholder}` 名称不匹配的条目，仅该条目回退到英文。TOML 格式错误时无法应用。设置在 macOS 保存到 `~/Library/Application Support/rs7z/settings.toml`，在 Linux 保存到 `${XDG_CONFIG_HOME:-~/.config}/rs7z/settings.toml`。可通过 `RS7Z_SETTINGS` 覆盖设置文件路径。
