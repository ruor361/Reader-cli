# Windows 构建环境

[返回 README](../README.md) · [开发指南](development.md)

仓库的 `rust-toolchain.toml` 选择 stable Rust，并包含 `rustfmt` 和 `clippy`；
不硬编码 Windows host 或 build target。所有命令均在包含 `Cargo.toml` 的项目目录执行。

## 选择工具链

| 路线 | 前置条件 | Cargo 命令 |
| --- | --- | --- |
| MSVC | Visual Studio Build Tools 的 C++ 构建工具和 Windows SDK | `cargo ...`，先确认当前 host 为 MSVC |
| GNU | 64 位 MinGW-w64 的 `gcc.exe`、`ar.exe` 在 `PATH` 中 | `cargo +stable-x86_64-pc-windows-gnu ...` |

在 PowerShell 中确认实际工具链：

```powershell
rustup show
cargo -vV
```

需要 MSVC 时，可显式使用 `cargo +stable-x86_64-pc-windows-msvc`。
以下介绍 x86_64 Windows 的 GNU 路线。无需修改仓库 target 或全局默认工具链；
[rustup 命令行覆盖](https://rust-lang.github.io/rustup/overrides.html)只作用于当次命令。

## 准备 MinGW-w64

检查已有工具链：

```powershell
where.exe gcc
where.exe ar
gcc --version
```

`gcc.exe` 和 `ar.exe` 应来自同一套 64 位工具链。没有安装时，可使用
[MSYS2 官方安装器](https://www.msys2.org/docs/installer/)。按照
[MSYS2 环境说明](https://www.msys2.org/docs/environments/)选择 UCRT64，
在 UCRT64 shell 中执行：

```sh
pacman -Syu
pacman -S --needed mingw-w64-ucrt-x86_64-toolchain
```

若更新要求关闭 shell，按提示重新打开后继续更新。
将 MSYS2 安装目录下的 `ucrt64\bin` 加入用户 `PATH`，默认路径通常为
`C:\msys64\ucrt64\bin`。重新打开 PowerShell 后再次检查 `gcc` 和 `ar`。
不要将不同架构或不同 C 运行时的工具和库混用。

## 安装 Rust GNU 工具链

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu `
  --profile minimal `
  --component rustfmt `
  --component clippy

cargo +stable-x86_64-pc-windows-gnu -vV
```

输出的 `host` 应为 `x86_64-pc-windows-gnu`。此后 GNU 命令均显式带上该工具链。

## 构建与检查

```powershell
cargo +stable-x86_64-pc-windows-gnu build --release
cargo +stable-x86_64-pc-windows-gnu run -- --help
cargo +stable-x86_64-pc-windows-gnu fmt --all -- --check
cargo +stable-x86_64-pc-windows-gnu clippy --all-targets -- -D warnings
cargo +stable-x86_64-pc-windows-gnu test
```

构建产物为 `target\release\reader.exe`。首次使用先创建并配置书架：

```powershell
New-Item -ItemType Directory -Path .\Books -Force | Out-Null
cargo +stable-x86_64-pc-windows-gnu run -- refresh .\Books
cargo +stable-x86_64-pc-windows-gnu run -- "C:\path\to\book.epub"
cargo +stable-x86_64-pc-windows-gnu run -- shelf
```

## 下载中断恢复

rustup 出现 TLS、`unexpected-eof` 等错误时，先重试原安装命令。
需要手动补齐下载缓存时，使用**本次错误信息**给出的官方
`static.rust-lang.org` URL 和 SHA-256；不要复用历史版本、文件名或哈希。

下载完成后，替换下列两个占位值，再执行：

```powershell
$packagePath = 'C:\path\to\downloaded-package.tar.xz'
$packageSha256 = '<SHA-256 from the current rustup error>'
if ($packageSha256 -notmatch '^[0-9a-fA-F]{64}$') {
  throw 'Replace the SHA-256 placeholder before continuing.'
}
$packageSha256 = $packageSha256.ToLowerInvariant()
$actualSha256 = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualSha256 -ne $packageSha256) {
  throw 'SHA-256 mismatch; download the package again.'
}

$taskRustupRoot = if ($env:RUSTUP_HOME) {
  $env:RUSTUP_HOME
} else {
  Join-Path $env:USERPROFILE '.rustup'
}
$taskDownloads = Join-Path $taskRustupRoot 'downloads'
New-Item -ItemType Directory -Path $taskDownloads -Force | Out-Null
Copy-Item -LiteralPath $packagePath -Destination (Join-Path $taskDownloads $packageSha256)

rustup toolchain install stable-x86_64-pc-windows-gnu `
  --profile minimal `
  --component rustfmt `
  --component clippy
```

缓存文件以 SHA-256 命名，不带扩展名。不要手动解压到工具链目录。

## 常见问题

- 找不到 `gcc` 或 `ar`：检查 `PATH`，重新打开 PowerShell，并用 `where.exe`
  确认两者来自同一套工具链。
- GNU 构建却报缺少 `cl.exe` 或 `link.exe`：用当次命令的 `cargo +... -vV`
  检查 host，再确认依赖是否另行调用 MSVC 工具。
- MSVC 构建报链接器或 SDK 缺失：检查 Visual Studio Build Tools 的 C++ 工作负载
  和 Windows SDK，必要时从 Developer PowerShell 执行。
- 模块文件缺失或 CLI 分支未实现：检查工作区已有修改；这是源码完整性问题。
