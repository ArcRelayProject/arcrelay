# Exercise packaging decisions in a temporary checkout, without a certificate,
# .NET SDK, Windows SDK, or modifying installed applications/registry entries.
$ErrorActionPreference = "Stop"
$fixture = Join-Path ([System.IO.Path]::GetTempPath()) ("arcrelay-share-test-" + [guid]::NewGuid().ToString("N"))
$scriptRoot = Join-Path $fixture "native/windows-share"
$output = Join-Path $fixture "binaries/windows-share"
$shell = (Get-Process -Id $PID).Path
$environmentNames = @(
    "ARCRELAY_WINDOWS_CERT_PFX", "ARCRELAY_WINDOWS_CERT_PASSWORD",
    "ARCRELAY_WINDOWS_CERT_THUMBPRINT", "ARCRELAY_WINDOWS_REQUIRE_SHARE_TARGET",
    "ARCRELAY_TEST_DOTNET_LOG"
)
$savedEnvironment = @{}

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

try {
    foreach ($name in $environmentNames) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
        [Environment]::SetEnvironmentVariable($name, $null, "Process")
    }
    New-Item -ItemType Directory -Force -Path $scriptRoot, $output | Out-Null
    Copy-Item (Join-Path $PSScriptRoot "build.ps1") $scriptRoot
    Copy-Item (Join-Path $PSScriptRoot "app.manifest.in") $scriptRoot
    Set-Content (Join-Path $output ".gitignore") "*"
    $env:ARCRELAY_TEST_DOTNET_LOG = Join-Path $fixture "dotnet-arguments.txt"
    $driver = Join-Path $fixture "driver.ps1"
    @'
param([switch]$CheckOnly, [switch]$RequireSignedPackage)
$ErrorActionPreference = "Stop"
function dotnet {
    [IO.File]::WriteAllLines($env:ARCRELAY_TEST_DOTNET_LOG, [string[]]$args)
    $global:LASTEXITCODE = 0
}
try {
    & (Join-Path $PSScriptRoot "native/windows-share/build.ps1") -CheckOnly:$CheckOnly -RequireSignedPackage:$RequireSignedPackage
} catch {
    Write-Output $_.Exception.Message
    exit 1
}
'@ | Set-Content -LiteralPath $driver -Encoding UTF8

    # An incremental unsigned build must remove a previous signed payload and
    # never invoke dotnet, otherwise hundreds of runtime files are still shipped.
    Set-Content (Join-Path $output "ArcRelay.SystemShare.msix") "stale package"
    New-Item -ItemType Directory -Path (Join-Path $output "runtime") | Out-Null
    Set-Content (Join-Path $output "runtime/stale.dll") "stale runtime"
    $result = & $shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $driver 2>&1
    Assert-Condition ($LASTEXITCODE -eq 0) "Unsigned fallback failed: $result"
    Assert-Condition (-not (Test-Path $env:ARCRELAY_TEST_DOTNET_LOG)) "Unsigned fallback invoked dotnet."
    Assert-Condition (@(Get-ChildItem -LiteralPath $output -Force).Count -eq 1) "Unsigned fallback retained a stale payload."
    Assert-Condition (Test-Path (Join-Path $output ".gitignore")) "Unsigned fallback deleted the tracked placeholder."

    $result = & $shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $driver -RequireSignedPackage 2>&1
    Assert-Condition ($LASTEXITCODE -ne 0) "Required signing silently fell back to an unsigned build."
    Assert-Condition (-not (Test-Path $env:ARCRELAY_TEST_DOTNET_LOG)) "Required signing was checked after invoking dotnet."

    $env:ARCRELAY_WINDOWS_REQUIRE_SHARE_TARGET = "1"
    $result = & $shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $driver 2>&1
    Assert-Condition ($LASTEXITCODE -ne 0) "The release signing requirement was ignored."
    [Environment]::SetEnvironmentVariable("ARCRELAY_WINDOWS_REQUIRE_SHARE_TARGET", $null, "Process")

    # Compile checks run without signing but cannot overwrite bundle resources.
    Set-Content (Join-Path $output "sentinel.txt") "keep existing bundle resources"
    $result = & $shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $driver -CheckOnly 2>&1
    Assert-Condition ($LASTEXITCODE -eq 0) "Compile-only check failed: $result"
    $arguments = @(Get-Content -LiteralPath $env:ARCRELAY_TEST_DOTNET_LOG)
    Assert-Condition ($arguments[0] -eq "build") "Compile-only check published a runtime."
    Assert-Condition (Test-Path (Join-Path $output "sentinel.txt")) "Compile-only check changed bundle resources."
    Write-Output "Windows share packaging checks passed (unsigned cleanup, signing requirements, compile-only isolation)."
} finally {
    foreach ($name in $environmentNames) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], "Process")
    }
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
