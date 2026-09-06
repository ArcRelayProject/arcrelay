param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [switch]$CheckOnly,
    [switch]$RequireSignedPackage
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$output = Join-Path $projectRoot "binaries/windows-share"
$certificatePath = $env:ARCRELAY_WINDOWS_CERT_PFX
$certificatePassword = $env:ARCRELAY_WINDOWS_CERT_PASSWORD
$certificateThumbprint = $env:ARCRELAY_WINDOWS_CERT_THUMBPRINT
$hasCertificate = $certificatePath -or $certificateThumbprint

# The helper has no usable activation entry without its signed identity package.
# Decide before publishing either runtime, and remove output from prior builds.
# Compile-only CI checks must never populate the installer's resource directory.
if (-not $CheckOnly) {
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    Get-ChildItem -LiteralPath $output -Force | Where-Object { $_.Name -ne ".gitignore" } | Remove-Item -Recurse -Force
    if (-not $hasCertificate) {
        if ($RequireSignedPackage -or $env:ARCRELAY_WINDOWS_REQUIRE_SHARE_TARGET -eq "1") {
            throw "Windows system Share requires ARCRELAY_WINDOWS_CERT_PFX or ARCRELAY_WINDOWS_CERT_THUMBPRINT. No Share Target payload was bundled."
        }
        Write-Warning "Windows system Share disabled: no signing certificate configured. Skipping the Share Target and its runtimes; only Share with ArcRelay is included."
        return
    }
}

$target = if ($env:TAURI_ENV_TARGET_TRIPLE) { $env:TAURI_ENV_TARGET_TRIPLE } else { $env:TARGET }
$rid = if ($target -like "aarch64-*") { "win-arm64" } else { "win-x64" }
$packageArchitecture = if ($rid -eq "win-arm64") { "arm64" } else { "x64" }
$publisher = if ($env:ARCRELAY_WINDOWS_PUBLISHER) { $env:ARCRELAY_WINDOWS_PUBLISHER } else { "CN=ArcRelay" }
$applicationManifest = Join-Path $env:TEMP ("arcrelay-share-app-" + [guid]::NewGuid().ToString("N") + ".manifest")
$escapedPublisher = [System.Security.SecurityElement]::Escape($publisher)
$applicationManifestText = (Get-Content (Join-Path $PSScriptRoot "app.manifest.in") -Raw).Replace("@PUBLISHER@", $escapedPublisher)
Set-Content -LiteralPath $applicationManifest -Value $applicationManifestText -Encoding UTF8

try {
    if ($CheckOnly) {
        dotnet build (Join-Path $PSScriptRoot "ArcRelay.ShareTarget.csproj") `
            --configuration $Configuration `
            --runtime $rid `
            -p:ApplicationManifest=$applicationManifest
        if ($LASTEXITCODE -ne 0) { throw "Share Target compile check failed with exit code $LASTEXITCODE." }
        return
    }
    dotnet publish (Join-Path $PSScriptRoot "ArcRelay.ShareTarget.csproj") `
        --configuration $Configuration `
        --runtime $rid `
        --self-contained true `
        --output $output `
        -p:ApplicationManifest=$applicationManifest
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed with exit code $LASTEXITCODE." }
} finally {
    if (Test-Path -LiteralPath $applicationManifest) { Remove-Item -LiteralPath $applicationManifest -Force }
}

Copy-Item (Join-Path $projectRoot "icons/icon.png") (Join-Path $output "ArcRelay.png") -Force

$applicationVersion = (Get-Content (Join-Path $projectRoot "tauri.conf.json") -Raw | ConvertFrom-Json).version
$versionParts = @($applicationVersion.Split('.'))
while ($versionParts.Count -lt 4) { $versionParts += "0" }
$packageVersion = ($versionParts[0..3] -join '.')
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
$makeAppx = Get-ChildItem -Path $sdkRoot -Filter makeappx.exe -Recurse |
    Sort-Object FullName -Descending | Select-Object -First 1
$signTool = Get-ChildItem -Path $sdkRoot -Filter signtool.exe -Recurse |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeAppx -or -not $signTool) {
    throw "Windows SDK MakeAppx.exe and SignTool.exe are required."
}

$staging = Join-Path $env:TEMP ("arcrelay-share-package-" + [guid]::NewGuid().ToString("N"))
try {
    $assets = Join-Path $staging "Assets"
    New-Item -ItemType Directory -Force -Path $assets | Out-Null
    $manifest = Get-Content (Join-Path $PSScriptRoot "AppxManifest.xml.in") -Raw
    $manifest = $manifest.Replace("@PROCESSOR_ARCHITECTURE@", $packageArchitecture)
    $manifest = $manifest.Replace("@PUBLISHER@", $escapedPublisher)
    $manifest = $manifest.Replace("@PACKAGE_VERSION@", $packageVersion)
    Set-Content -LiteralPath (Join-Path $staging "AppxManifest.xml") -Value $manifest -Encoding UTF8

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

    $packagePath = Join-Path $output "ArcRelay.SystemShare.msix"
    & $makeAppx.FullName pack /d $staging /p $packagePath /nv /o
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed with exit code $LASTEXITCODE." }
    if ($certificatePath) {
        & $signTool.FullName sign /fd SHA256 /f $certificatePath /p $certificatePassword $packagePath
    } else {
        & $signTool.FullName sign /fd SHA256 /sha1 $certificateThumbprint $packagePath
    }
    if ($LASTEXITCODE -ne 0) { throw "SignTool failed with exit code $LASTEXITCODE." }
    $payloadBytes = (Get-ChildItem -LiteralPath $output -File -Recurse | Measure-Object -Property Length -Sum).Sum
    Write-Output ("Windows system Share enabled: signed identity {0}; payload {1:N2} MiB." -f $packageVersion, ($payloadBytes / 1MB))
} finally {
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
}
