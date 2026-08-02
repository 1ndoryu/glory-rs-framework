param(
    [string[]]$TargetDirs = @('C:\tmp\glory-target', 'C:\tmp\glory-openapi-target'),
    [string[]]$ExcludeDirs = @(),
    [int]$MaxTotalMB = 15360,
    [int]$IntervalSeconds = 120,
    [switch]$AllowCleanupWhileBuildActive
)

$ErrorActionPreference = 'Stop'
$cleanScript = Join-Path $PSScriptRoot 'clean-cargo-target.ps1'

while ($true) {
    Start-Sleep -Seconds $IntervalSeconds
    $cleanArgs = @(
        '-ExecutionPolicy', 'Bypass',
        '-File', $cleanScript,
        '-TargetDirs', $TargetDirs,
        '-ExcludeDirs', $ExcludeDirs,
        '-MaxTotalMB', $MaxTotalMB
    )
    if ($AllowCleanupWhileBuildActive) {
        $cleanArgs += '-AllowCleanupWhileBuildActive'
    }
    & powershell @cleanArgs
}