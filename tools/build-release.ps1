#Requires -Version 5.1
<#
.SYNOPSIS
    构建 CapsX release 与 Inno Setup 安装包。

.DESCRIPTION
    依次执行格式检查、clippy、测试、release 构建，然后调用 ISCC。
    -Full 会额外运行需要 runtime\kanata.exe 的集成测试。

.NOTES
    PowerShell 5.1 对无 BOM UTF-8 会乱码；本文件必须保持 UTF-8 with BOM。
#>
param(
    [switch]$Full
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments
    )
    Write-Host "`n> $File $($Arguments -join ' ')" -ForegroundColor Cyan
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "命令失败（exit $LASTEXITCODE）：$File $($Arguments -join ' ')"
    }
}

if (-not (Test-Path 'runtime\kanata.exe')) {
    Invoke-Checked 'powershell' '-NoProfile' '-ExecutionPolicy' 'Bypass' '-File' 'tools\fetch-kanata.ps1'
}

Invoke-Checked 'cargo' 'fmt' '--all' '--check'
Invoke-Checked 'cargo' 'clippy' '--locked' '--all-targets' '--' '-D' 'warnings'
Invoke-Checked 'cargo' 'test' '--locked'
if ($Full) {
    Invoke-Checked 'cargo' 'test' '--locked' '-p' 'capsx-core' '--test' 'config_snapshots' '--' '--ignored'
}
Invoke-Checked 'cargo' 'build' '--locked' '--release' '-p' 'capsx-app'

$iscc = @(
    (Get-Command 'iscc.exe' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    'C:\Program Files (x86)\Inno Setup 6\ISCC.exe',
    'C:\Program Files\Inno Setup 6\ISCC.exe'
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

if (-not $iscc) {
    throw '找不到 Inno Setup 6 的 ISCC.exe，请先安装 Inno Setup 6。'
}

Invoke-Checked $iscc 'installer\capsx.iss'

$exe = Get-Item 'target\release\capsx.exe'
$installer = Get-ChildItem 'dist\CapsX-Setup-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Host "`n构建完成：" -ForegroundColor Green
Write-Host ("  capsx.exe : {0:N0} 字节 ({1:N2} MiB)" -f $exe.Length, ($exe.Length / 1MB))
if ($installer) {
    Write-Host ("  installer : {0:N0} 字节 ({1:N2} MiB) -> {2}" -f $installer.Length, ($installer.Length / 1MB), $installer.FullName)
}
