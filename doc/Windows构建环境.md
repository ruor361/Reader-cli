# Windows 构建环境

仓库内的 `rust-toolchain.toml` 只选择宿主平台的 stable Rust，不再硬编码 Windows host
或 build target。Linux、macOS 和 Windows 都默认本机构建，不会因为仓库配置而被迫交叉
编译。

Windows 可以使用 MSVC 或 GNU：安装了 Visual Studio Build Tools 时可直接运行裸
`cargo`；不想依赖 Visual Studio、Windows SDK 或 `link.exe` 时，可以显式使用
`x86_64-pc-windows-gnu`。以下内容是 GNU 路线，不会改变其他平台或仓库默认 target。

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

进入仓库后确认 GNU 工具链结果。由于仓库不再强制 Windows GNU，每条 GNU 构建命令都
显式带上完整 toolchain 名称：

```powershell
cargo +stable-x86_64-pc-windows-gnu -vV
```

输出中的 `host` 应为 `x86_64-pc-windows-gnu`。无需修改全局默认工具链；裸 `cargo`
仍使用当前 Windows 默认 host，显式的 `+stable-x86_64-pc-windows-gnu` 只作用于当次命令。

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
cargo +stable-x86_64-pc-windows-gnu run -- --help
cargo +stable-x86_64-pc-windows-gnu fmt --all -- --check
cargo +stable-x86_64-pc-windows-gnu clippy --all-targets -- -D warnings
cargo +stable-x86_64-pc-windows-gnu test
```

打开实际书籍：

```powershell
cargo +stable-x86_64-pc-windows-gnu run -- "C:\path\to\book.epub"
cargo +stable-x86_64-pc-windows-gnu run -- shelf
```

如果 GNU 命令报 `gcc` 或 `ar` 找不到，先修复 `PATH`。如果错误提到 `cl.exe` 或
`link.exe`，通常表示误用了裸 `cargo`；用
`cargo +stable-x86_64-pc-windows-gnu -vV` 检查当次 host。选择 MSVC 路线的开发者则应
安装 Visual Studio Build Tools，并继续使用裸 `cargo`。
