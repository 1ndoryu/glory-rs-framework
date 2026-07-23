param(
    [string[]]$TargetDirs = @('C:\tmp\glory-target'),
    [string[]]$ExcludeDirs = @(),
    [int]$MaxTotalMB = 15360,
    [int]$IntervalSeconds = 120
)

$ErrorActionPreference = 'Stop'
$cleanScript = Join-Path $PSScriptRoot 'clean-cargo-target.ps1'

while ($true) {
    Start-Sleep -Seconds $IntervalSeconds
    & powershell -ExecutionPolicy Bypass -File $cleanScript -TargetDirs $TargetDirs -ExcludeDirs $ExcludeDirs -MaxTotalMB $MaxTotalMB
}