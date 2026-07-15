# Windows 构建环境

本项目在 Windows 上统一使用 `x86_64-pc-windows-gnu` Rust 主机工具链。仓库内的
`rust-toolchain.toml` 会让裸 `cargo` 命令自动选择它；不依赖 Visual Studio、Windows
SDK 或 `link.exe`。

## 前置检查

在 PowerShell 中执行：

```powershell
where.exe rustup
where.exe gcc
where.exe ar
gcc --version
```

`gcc.exe` 和 `ar.exe` 必须来自同一套 64 位 MinGW-w64 工具链并位于 `PATH`。当前开发机
已验证的工具链是 MinGW-w64 GCC 15.2.0，目录为
`C:\ProgramData\mingw64\mingw64\bin`。

如果尚未安装 MinGW-w64，推荐使用 [MSYS2 官方安装器](https://www.msys2.org/docs/installer/)。
在 MSYS2 UCRT64 shell 中安装官方工具链组：

```sh
pacman -Syu
pacman -S --needed mingw-w64-ucrt-x86_64-toolchain
```

安装后把 `C:\msys64\ucrt64\bin` 加入 Windows 用户 `PATH`，重新打开 PowerShell，再次
运行上述前置检查。MSYS2 官方建议新项目使用 UCRT64 环境。

## 安装 Rust GNU 工具链

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu `
  --profile minimal `
  --component rustfmt `
  --component clippy
```

进入仓库后确认自动选择结果：

```powershell
rustup show active-toolchain
cargo -vV
```

`cargo -vV` 的 `host` 应为 `x86_64-pc-windows-gnu`。无需修改全局默认工具链，也无需在
每条命令后附加 `+stable-gnu`。

## rustup 下载中断恢复

如果 rustup 报 `peer closed connection without sending TLS close_notify`、
`unexpected-eof` 或类似网络错误，不要解压压缩包，也不要从第三方镜像替换二进制。
使用错误信息中给出的 `static.rust-lang.org` 官方 URL 下载对应文件，并核对 rustup 给出的
SHA-256。

2026-07-14 验证过的当前包如下：

- 文件：`rustc-1.97.0-x86_64-pc-windows-gnu.tar.xz`
- 官方 URL：<https://static.rust-lang.org/dist/2026-07-09/rustc-1.97.0-x86_64-pc-windows-gnu.tar.xz>
- SHA-256：`c5b717977488eb02b56fa89c3629a7b7ebdaf1c6aecc3a6897cb924aa128c138`

假设文件下载到了 `E:\Downloads`，执行：

```powershell
$source = "E:\Downloads\rustc-1.97.0-x86_64-pc-windows-gnu.tar.xz"
$expected = "c5b717977488eb02b56fa89c3629a7b7ebdaf1c6aecc3a6897cb924aa128c138"
$actual = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()

if ($actual -ne $expected) {
  throw "SHA-256 mismatch; delete the incomplete file and download it again."
}

$downloads = Join-Path $env:USERPROFILE ".rustup\downloads"
New-Item -ItemType Directory -Path $downloads -Force | Out-Null
Copy-Item -LiteralPath $source -Destination (Join-Path $downloads $expected) -Force

rustup toolchain install stable-x86_64-pc-windows-gnu `
  --profile minimal `
  --component rustfmt `
  --component clippy
```

rustup 缓存中的文件名就是 SHA-256，不带扩展名。重试时 rustup 会再次校验缓存；校验通过
后直接安装，缺失或错误的缓存不会被静默接受。若将来 stable 版本变化，应使用当次 rustup
错误信息中的 URL、文件名和哈希，不要沿用上面的 1.97.0 数值。

## 构建与验收

```powershell
cargo run -- --help
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

打开实际书籍：

```powershell
cargo run -- "C:\path\to\book.epub"
cargo run -- shelf
```

如果报 `gcc` 或 `ar` 找不到，先修复 `PATH`；如果错误再次提到 `link.exe`，用
`cargo -vV` 检查当前 host。只要仓库中的 `rust-toolchain.toml` 生效，host 就应是 GNU，
不应回落到 MSVC。
