param(
    [Parameter(Mandatory = $true)][string]$ExternalLocation,
    [Parameter(Mandatory = $true)][string]$PackagePath
)

$ErrorActionPreference = "Stop"
$logDirectory = Join-Path ([Environment]::GetFolderPath("ApplicationData")) "ArcRelay/system-share"
New-Item -ItemType Directory -Force -Path $logDirectory | Out-Null
$logPath = Join-Path $logDirectory "registration.log"
function Write-RegistrationLog([string]$Message) {
    Add-Content -LiteralPath $logPath -Value ("{0:o} {1}" -f [DateTime]::UtcNow, $Message) -Encoding UTF8
    Write-Output $Message
}

try {
    # Read the actual build; unmanifested PowerShell hosts can report Windows 8.
    $windowsBuild = [int](Get-ItemPropertyValue "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -Name CurrentBuildNumber)
    if ($windowsBuild -lt 19041) {
        Write-RegistrationLog "System Share requires Windows build 19041 or newer; Share with ArcRelay remains available."
        exit 0
    }
    if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) {
        Write-RegistrationLog "No signed Share Target package was included; Share with ArcRelay remains available."
        exit 0
    }
    $helperPath = Join-Path $ExternalLocation "system-share/windows/ArcRelay.ShareTarget.exe"
    if (-not (Test-Path -LiteralPath $helperPath -PathType Leaf)) {
        throw "Share Target executable is missing: $helperPath"
    }
    Add-AppxPackage -Path $PackagePath -ExternalLocation $ExternalLocation -ForceUpdateFromAnyVersion -ForceApplicationShutdown -ErrorAction Stop
    $package = Get-AppxPackage -Name "ArcRelay.SystemShare" -ErrorAction Stop
    if (-not $package) { throw "Share Target registration completed without an installed package." }
    Write-RegistrationLog "Registered $($package.PackageFullName) with external location $ExternalLocation."
} catch {
    Write-RegistrationLog "System Share registration failed: $($_.Exception.Message)"
    exit 1
}
