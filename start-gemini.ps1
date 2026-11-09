# Starts Gemini CLI in this project, with the Gemini API key and the Fly deploy token loaded for this window only.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$env:GEMINI_API_KEY = [Environment]::GetEnvironmentVariable('GEMINI_API_KEY', 'User')
$creds = Join-Path $HOME 'Downloads\BigBoysVault-handoff\02-CREDENTIALS-PRIVATE.md'
if (Test-Path $creds) {
  $m = [regex]::Match((Get-Content $creds -Raw), 'FlyV1 [^\r\n`]+')
  if ($m.Success) { $env:FLY_API_TOKEN = $m.Value.Trim(); Write-Host 'Fly deploy token loaded.' }
} else { Write-Host "No credentials file at $creds; deploying will need 'fly auth login'." }
gemini
