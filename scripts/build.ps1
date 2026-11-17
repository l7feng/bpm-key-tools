# 构建 BPM 调性识别（使用本机便携 Rust 工具链，不依赖系统 PATH）
# 用法：在 PowerShell 中执行  powershell -ExecutionPolicy Bypass -File scripts\build.ps1
#
# 退出码：0 = 构建成功且 exe 已更新；非 0 = 任一步失败（失败即停，不再"npx 失败仍报成功"）。

$ErrorActionPreference = "Stop"

$env:PATH = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\cargo\bin;" + $env:PATH
$env:RUSTUP_HOME = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\rustup"
$env:CARGO_HOME = "D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\cargo"

Set-Location (Split-Path $PSScriptRoot -Parent)

Write-Host "cargo: $(cargo --version 2>&1)"
Write-Host "rustc: $(rustc --version 2>&1)"

# exe 被运行中的进程占用时链接会报 os error 5，先关掉再构建
Get-Process -Name 'bpm-key-tool' -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "停止运行中的 bpm-key-tool (PID $($_.Id))"
    try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch { Write-Host "停止失败：$($_.Exception.Message)" }
}

$exe = "src-tauri\target\release\bpm-key-tool.exe"
$before = if (Test-Path -LiteralPath $exe) { (Get-Item -LiteralPath $exe).LastWriteTimeUtc } else { $null }

# --no-bundle：只产出 exe，不生成 NSIS/MSI 安装包
# 原生进程的失败不会触发 $ErrorActionPreference，必须显式看退出码
$log = Join-Path $env:TEMP "bpm-key-build.log"
& npx tauri build --no-bundle 2>&1 | Tee-Object -FilePath $log
$code = $LASTEXITCODE
if ($code -ne 0) {
    Write-Host "构建失败：npx tauri build 退出码 $code（日志 $log）"
    Get-Content -LiteralPath $log -Tail 30
    exit $code
}

if (-not (Test-Path -LiteralPath $exe)) {
    Write-Host "构建失败：未找到产物 $exe（npx 返回 0 但 exe 不存在）"
    exit 1
}

$item = Get-Item -LiteralPath $exe
if ($before -and $item.LastWriteTimeUtc -le $before) {
    Write-Host "构建存疑：exe 时间戳未更新（$($item.LastWriteTime)），请确认 tauri 是否真的重编了 Rust 侧"
}

$size = [math]::Round($item.Length / 1MB, 2)
$sha = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
Write-Host "构建成功：$exe ($size MB, $($item.Length) B)"
Write-Host "SHA256: $sha"
exit 0
