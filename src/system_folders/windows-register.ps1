$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$service = Get-Service -Name WebClient -ErrorAction SilentlyContinue
if ($null -eq $service) {
    throw 'Windows WebClient is not installed. Enable the WebDAV client component to use Explorer network folders.'
}
if ($service.StartType -eq 'Disabled') {
    throw 'Windows WebClient is disabled. Enable it in Services, then retry.'
}
# Shell access can trigger a demand-start service without elevating ArcRelay.
# Do not modify machine service settings, authentication policy or file limits.
if (-not (Test-Path -LiteralPath $env:ARCRELAY_FOLDER_TARGET)) {
    throw 'Cannot reach the network folder. Start the Windows WebClient service in Services and confirm the remote device is online, then retry.'
}
$root = [Environment]::GetFolderPath([Environment+SpecialFolder]::NetworkShortcuts)
if ([string]::IsNullOrEmpty($root)) { throw 'Windows Network Shortcuts folder is unavailable.' }
$folder = Join-Path $root ('ArcRelay-' + $env:ARCRELAY_FOLDER_ID)
[IO.Directory]::CreateDirectory($folder) | Out-Null
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $folder 'target.lnk'))
$shortcut.TargetPath = $env:ARCRELAY_FOLDER_TARGET
$shortcut.Description = $env:ARCRELAY_FOLDER_NAME
$shortcut.IconLocation = "$env:SystemRoot\System32\shell32.dll,9"
$shortcut.Save()
# Use the Shell network-location class so the item appears under This PC.
# Display names are data, and cannot inject extra desktop.ini entries.
$name = $env:ARCRELAY_FOLDER_NAME -replace '[\r\n\x00]', ' '
$ini = "[.ShellClassInfo]`r`nCLSID2={0AFACED1-E828-11D1-9187-B532F1E9575D}`r`nFlags=2`r`nLocalizedResourceName=$name`r`n"
$iniPath = Join-Path $folder 'desktop.ini'
if (Test-Path -LiteralPath $iniPath) { [IO.File]::SetAttributes($iniPath, [IO.FileAttributes]::Normal) }
[IO.File]::WriteAllText($iniPath, $ini, [Text.Encoding]::Unicode)
[IO.File]::SetAttributes($iniPath, [IO.FileAttributes]::Hidden -bor [IO.FileAttributes]::System)
[IO.File]::SetAttributes($folder, [IO.FileAttributes]::ReadOnly)
