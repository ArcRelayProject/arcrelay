$ErrorActionPreference = "SilentlyContinue"
Get-AppxPackage -Name "ArcRelay.SystemShare" | Remove-AppxPackage
