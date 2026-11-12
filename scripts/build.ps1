# 构建 BPM 调性识别（使用本机便携 Rust 工具链，不依赖系统 PATH）
# 用法：在 PowerShell 中执行  powershell -ExecutionPolicy Bypass -File scripts\build.ps1

$ErrorActionPreference = "Stop"

$env:PATH = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\cargo\bin;" + $env:PATH
$env:RUSTUP_HOME = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\rustup"
$env:CARGO_HOME = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\cargo"

Set-Location (Split-Path $PSScriptRoot -Parent)

Write-Host "cargo: $(cargo --version)"
Write-Host "rustc: $(rustc --version)"

# --no-bundle：只产出 exe，不生成 NSIS/MSI 安装包
npx tauri build --no-bundle

$exe = "src-tauri\target\release\bpm-key-tool.exe"
if (Test-Path $exe) {
    $size = [math]::Round((Get-Item $exe).Length / 1MB, 2)
    Write-Host "构建成功：$exe ($size MB)"
} else {
    Write-Error "未找到产物 $exe"
}
