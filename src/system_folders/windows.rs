//! Per-user Explorer network locations; no administrator rights or drive-letter
//! allocation. PowerShell receives all variable data through the environment.
use super::*;
use webdav::Endpoint;

async fn powershell(script: &str, view: &SystemFolder, target: &str) -> Result<(), Error> {
    let mut command = tokio::process::Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("ARCRELAY_FOLDER_ID", &view.id)
        .env("ARCRELAY_FOLDER_NAME", &view.name)
        .env("ARCRELAY_FOLDER_TARGET", target)
        .creation_flags(0x08000000)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| {
            Error::unavailable(
                "Explorer integration timed out; check the Windows WebClient service",
            )
        })??;
    if output.status.success() {
        return Ok(());
    }
    // Never include stdout or the target: the UNC path contains a capability.
    let mut error = String::from_utf8_lossy(&output.stderr).into_owned();
    if !target.is_empty() {
        error = error.replace(target, "[system folder]");
    }
    Err(Error::unavailable(format!(
        "Windows Explorer integration failed: {}",
        error.trim()
    )))
}

async fn target(root: &Path, view: &SystemFolder) -> Result<String, Error> {
    uuid::Uuid::parse_str(&view.id).map_err(|_| Error::unavailable("invalid folder identity"))?;
    let endpoint: Endpoint =
        serde_json::from_slice(&tokio::fs::read(root.join("webdav.json")).await?)
            .map_err(|e| Error::unavailable(e.to_string()))?;
    Ok(format!(
        r"\\127.0.0.1@{}\DavWWWRoot\{}\{}",
        endpoint.port, endpoint.token, view.id
    ))
}

pub(super) async fn register(root: &Path, view: &SystemFolder) -> Result<(), Error> {
    let target = target(root, view).await?;
    powershell(include_str!("windows-register.ps1"), view, &target).await
}

pub(super) async fn open(root: &Path, view: &SystemFolder) -> Result<(), Error> {
    let target = target(root, view).await?;
    tokio::process::Command::new("explorer.exe")
        .arg(target)
        .spawn()?;
    Ok(())
}

pub(super) async fn remove(view: &SystemFolder) -> Result<(), Error> {
    powershell(
        r#"
$ErrorActionPreference = 'Stop'
$root = [Environment]::GetFolderPath([Environment+SpecialFolder]::NetworkShortcuts)
$folder = Join-Path $root ('ArcRelay-' + $env:ARCRELAY_FOLDER_ID)
if (Test-Path -LiteralPath $folder) {
    # Remove only our shortcut metadata. Never recurse into the network target.
    foreach ($name in @('target.lnk', 'desktop.ini')) {
        $file = Join-Path $folder $name
        if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force }
    }
    [IO.Directory]::Delete($folder)
}
"#,
        view,
        "",
    )
    .await
}
