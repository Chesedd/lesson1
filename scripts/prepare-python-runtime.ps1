param(
  [Parameter(Mandatory=$true)][string]$PythonArchive,
  [Parameter(Mandatory=$true)][string]$PythonArchiveSha256,
  [Parameter(Mandatory=$true)][string]$Wheelhouse
)
$ErrorActionPreference = 'Stop'
$ExpectedPython = '3.12.10'
$ExpectedPandas = '2.2.3'
$Root = Join-Path $PSScriptRoot '../frontend/src-tauri/runtime/windows-x86_64'
if ((Get-FileHash $PythonArchive -Algorithm SHA256).Hash -ne $PythonArchiveSha256) { throw 'Python archive SHA-256 mismatch' }
Remove-Item $Root -Recurse -Force -ErrorAction SilentlyContinue
New-Item $Root -ItemType Directory | Out-Null
Expand-Archive $PythonArchive $Root
# Wheels must be supplied by the trusted build pipeline; installation never occurs on an end-user machine.
& (Join-Path $Root 'python.exe') -I -B -m pip install --no-index --find-links $Wheelhouse --only-binary=:all: "pandas==$ExpectedPandas"
$Version = & (Join-Path $Root 'python.exe') -I -B -c 'import platform; print(platform.python_version())'
$Pandas = & (Join-Path $Root 'python.exe') -I -B -c 'import pandas; print(pandas.__version__)'
if ($Version -ne $ExpectedPython -or $Pandas -ne $ExpectedPandas) { throw 'Runtime version mismatch' }
$Inventory = [ordered]@{
  python_version = $Version
  pandas_version = $Pandas
  python_exe_sha256 = (Get-FileHash (Join-Path $Root 'python.exe') -Algorithm SHA256).Hash.ToLower()
  python_dll_sha256 = (Get-FileHash (Join-Path $Root 'python312.dll') -Algorithm SHA256).Hash.ToLower()
}
$InventoryJson = $Inventory | ConvertTo-Json
$Utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $Root 'runtime-inventory.json'), $InventoryJson, $Utf8WithoutBom)
Write-Host "Prepared offline runtime at $Root"
