#Requires -Version 5.1
<#
.SYNOPSIS
    下载并准备 CapsX 的 kanata 内核（tty + winIOv2 变体）。

.DESCRIPTION
    - 从 GitHub Release 下载 windows-binaries-x64.zip
    - 校验 SHA256
    - 取出 kanata_windows_tty_winIOv2_x64.exe，重命名为 runtime\kanata.exe
    - 写入 runtime\kanata.version
    - 附带 kanata 的 LICENSE（LGPL-3.0），分发时必须一起打包

.NOTES
    只使用 tty 变体：gui 变体会自带托盘图标，与 CapsX 的托盘冲突。
    只使用 winIOv2：wintercept 需要驱动，且有已知的蓝牙键盘失灵问题。
#>
param(
    [string]$Version = '1.12.0',
    [string]$Sha256  = '13947ed78cfa3284bfef854e3c542c74ab366236b72fd9f7e039f8638deead9d',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$root    = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $root 'runtime'
$target  = Join-Path $runtime 'kanata.exe'

New-Item -ItemType Directory -Path $runtime -Force | Out-Null

if ((Test-Path $target) -and (-not $Force)) {
    Write-Host "runtime\kanata.exe 已存在，跳过下载（加 -Force 可强制重下）。"
    exit 0
}

$asset = 'windows-binaries-x64.zip'
$url   = "https://github.com/jtroo/kanata/releases/download/v$Version/$asset"
$tmp   = Join-Path ([System.IO.Path]::GetTempPath()) "capsx-kanata-$Version-$asset"

Write-Host "下载 kanata v$Version ..."
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $tmp -TimeoutSec 300

$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash.ToLowerInvariant()
if ($actual -ne $Sha256.ToLowerInvariant()) {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
    throw "SHA256 校验失败：期望 $Sha256，实际 $actual"
}
Write-Host "SHA256 校验通过。"

$extract = Join-Path ([System.IO.Path]::GetTempPath()) "capsx-kanata-$Version-extract"
if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
Expand-Archive -LiteralPath $tmp -DestinationPath $extract -Force

$exe = Get-ChildItem $extract -Filter 'kanata_windows_tty_winIOv2_x64.exe' -Recurse |
       Select-Object -First 1
if (-not $exe) { throw '压缩包里没有找到 kanata_windows_tty_winIOv2_x64.exe' }

Copy-Item $exe.FullName $target -Force
Set-Content -LiteralPath (Join-Path $runtime 'kanata.version') -Value $Version -Encoding ASCII -NoNewline

Write-Host "已写入：$target"
Write-Host "版本：$Version"

try {
    $lic = Join-Path $runtime 'LICENSE.kanata.txt'
    Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/jtroo/kanata/v$Version/LICENSE" -OutFile $lic -TimeoutSec 60
    Write-Host "已下载 kanata LICENSE：$lic"
} catch {
    Write-Warning "LICENSE 下载失败（不影响使用，但分发前请手工补上 LGPL-3.0 文本）：$($_.Exception.Message)"
}

try { Remove-Item $tmp -Force -ErrorAction Stop } catch { }
try { Remove-Item $extract -Recurse -Force -ErrorAction Stop } catch { }

Write-Host '完成。'

