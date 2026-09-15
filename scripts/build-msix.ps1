param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
    [string]$Target,
    [Parameter(Mandatory = $true)][string]$IdentityName,
    [Parameter(Mandatory = $true)][string]$Publisher,
    [Parameter(Mandatory = $true)][string]$PublisherDisplayName,
    [string]$PackageVersion,
    [string]$OutputDirectory = "dist/msix"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function ConvertTo-XmlText([string]$Value) {
    return [System.Security.SecurityElement]::Escape($Value)
}

function Resolve-PackageVersion([string]$RequestedVersion) {
    if (-not $RequestedVersion) {
        $applicationVersion = (Get-Content (Join-Path $projectRoot "tauri.conf.json") -Raw | ConvertFrom-Json).version
        if ($applicationVersion -notmatch '^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$') {
            throw "Tauri version '$applicationVersion' cannot be converted to a Microsoft Store package version."
        }
        # An MSIX major version cannot be zero. Keeping a fixed +1 offset makes
        # pre-1.0 SemVer releases valid without breaking ordering at ArcRelay 1.0.
        $parts = @(
            ([int]$Matches[1]) + 1
            [int]$Matches[2]
            [int]$Matches[3]
            0
        )
    } else {
        if ($RequestedVersion -notmatch '^(\d+)\.(\d+)\.(\d+)\.(\d+)$') {
            throw "PackageVersion must use four numeric parts, for example 1.2.3.0."
        }
        $parts = @(
            [int]$Matches[1]
            [int]$Matches[2]
            [int]$Matches[3]
            [int]$Matches[4]
        )
    }

    if ($parts[0] -eq 0) { throw "The Microsoft Store package major version cannot be zero." }
    if ($parts[3] -ne 0) { throw "The fourth package version part is reserved by Microsoft Store and must be zero." }
    foreach ($part in $parts) {
        if ($part -lt 0 -or $part -gt 65535) { throw "Every package version part must be between 0 and 65535." }
    }
    return ($parts -join '.')
}

function Copy-PackageFile([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Required MSIX file is missing: $Source" }
    $parent = Split-Path -Parent $Destination
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Copy-PackageDirectory([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Container)) { throw "Required MSIX directory is missing: $Source" }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -Path (Join-Path $Source '*') -Destination $Destination -Recurse -Force
}

if ($IdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') {
    throw "IdentityName must be the 3-50 character Package/Identity/Name value from Partner Center."
}
if ([string]::IsNullOrWhiteSpace($Publisher)) { throw "Publisher must be the Package/Identity/Publisher value from Partner Center." }
if ([string]::IsNullOrWhiteSpace($PublisherDisplayName)) { throw "PublisherDisplayName is required." }

$resolvedVersion = Resolve-PackageVersion $PackageVersion
$architecture = if ($Target.StartsWith('aarch64-')) { 'arm64' } else { 'x64' }
$mainExecutableCandidates = @(
    (Join-Path $projectRoot "target/$Target/release/arcrelay-desktop.exe"),
    (Join-Path $projectRoot "../target/$Target/release/arcrelay-desktop.exe")
)
if ($env:CARGO_TARGET_DIR) {
    $cargoTargetRoot = if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        $env:CARGO_TARGET_DIR
    } else {
        Join-Path $projectRoot $env:CARGO_TARGET_DIR
    }
    $mainExecutableCandidates = @((Join-Path $cargoTargetRoot "$Target/release/arcrelay-desktop.exe")) + $mainExecutableCandidates
}
$mainExecutable = $mainExecutableCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $mainExecutable) {
    throw "Compiled ArcRelay executable was not found for $Target. Checked: $($mainExecutableCandidates -join ', ')"
}
$makeAppxRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
$makeAppx = Get-ChildItem -Path $makeAppxRoot -Filter makeappx.exe -Recurse |
    Where-Object { $_.FullName -match '[\\/]x64[\\/]makeappx\.exe$' } |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if (-not $makeAppx) { throw "Windows SDK MakeAppx.exe (x64) is required." }

$staging = Join-Path $env:TEMP ("arcrelay-msix-" + [guid]::NewGuid().ToString("N"))
try {
    New-Item -ItemType Directory -Force -Path $staging | Out-Null
    Copy-PackageFile $mainExecutable (Join-Path $staging "ArcRelay.exe")
    Copy-PackageFile (Join-Path $projectRoot "binaries/sniptra-$Target.exe") (Join-Path $staging "sniptra.exe")
    Copy-PackageFile (Join-Path $projectRoot "binaries/sniptra-ocr-worker-$Target.exe") (Join-Path $staging "sniptra-ocr-worker.exe")

    Copy-PackageFile (Join-Path $projectRoot "frontend/src/assets/fonts/OFL.txt") (Join-Path $staging "licenses/NotoSansSC-OFL.txt")
    Copy-PackageFile (Join-Path $projectRoot "icons/icon-macos-1024.png") (Join-Path $staging "icons/icon-macos-1024.png")
    Copy-PackageFile (Join-Path $projectRoot "icons/icon-drag-preview.png") (Join-Path $staging "icons/icon-drag-preview.png")
    Copy-PackageDirectory (Join-Path $projectRoot "binaries/sniptra-notices") (Join-Path $staging "sniptra-notices")
    Copy-PackageDirectory (Join-Path $projectRoot "binaries/windows-share") (Join-Path $staging "system-share/windows")
    Get-ChildItem -LiteralPath (Join-Path $staging "system-share/windows") -Force |
        Where-Object { $_.Name -eq '.gitignore' -or $_.Extension -eq '.msix' } |
        Remove-Item -Recurse -Force

    $assets = Join-Path $staging "Assets"
    New-Item -ItemType Directory -Force -Path $assets | Out-Null
    Add-Type -AssemblyName System.Drawing
    $sourceImage = [System.Drawing.Image]::FromFile((Join-Path $projectRoot "icons/icon.png"))
    try {
        foreach ($asset in @(
            @{ Name = "StoreLogo.png"; Width = 50; Height = 50 },
            @{ Name = "Square44x44Logo.png"; Width = 44; Height = 44 },
            @{ Name = "Square150x150Logo.png"; Width = 150; Height = 150 },
            @{ Name = "Wide310x150Logo.png"; Width = 310; Height = 150 }
        )) {
            $bitmap = New-Object System.Drawing.Bitmap($asset.Width, $asset.Height)
            try {
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                try {
                    $graphics.Clear([System.Drawing.Color]::Transparent)
                    $side = [Math]::Min($asset.Width, $asset.Height)
                    $x = [Math]::Floor(($asset.Width - $side) / 2)
                    $y = [Math]::Floor(($asset.Height - $side) / 2)
                    $graphics.DrawImage($sourceImage, $x, $y, $side, $side)
                } finally { $graphics.Dispose() }
                $bitmap.Save((Join-Path $assets $asset.Name), [System.Drawing.Imaging.ImageFormat]::Png)
            } finally { $bitmap.Dispose() }
        }
    } finally { $sourceImage.Dispose() }

    $manifest = Get-Content (Join-Path $projectRoot "installer/msix/AppxManifest.xml.in") -Raw
    $manifest = $manifest.Replace("@IDENTITY_NAME@", (ConvertTo-XmlText $IdentityName))
    $manifest = $manifest.Replace("@PUBLISHER@", (ConvertTo-XmlText $Publisher))
    $manifest = $manifest.Replace("@PUBLISHER_DISPLAY_NAME@", (ConvertTo-XmlText $PublisherDisplayName))
    $manifest = $manifest.Replace("@PACKAGE_VERSION@", $resolvedVersion)
    $manifest = $manifest.Replace("@PROCESSOR_ARCHITECTURE@", $architecture)
    Set-Content -LiteralPath (Join-Path $staging "AppxManifest.xml") -Value $manifest -Encoding UTF8

    $resolvedOutputDirectory = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
        $OutputDirectory
    } else {
        Join-Path $projectRoot $OutputDirectory
    }
    New-Item -ItemType Directory -Force -Path $resolvedOutputDirectory | Out-Null
    $packagePath = Join-Path $resolvedOutputDirectory "ArcRelay_${resolvedVersion}_${architecture}.msix"
    & $makeAppx.FullName pack /v /h SHA256 /d $staging /p $packagePath /o
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed with exit code $LASTEXITCODE." }
    $hash = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Output "Created unsigned Microsoft Store package: $packagePath"
    Write-Output "SHA256: $hash"
} finally {
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
}
