param(
    [string[]]$TargetDirs = @('C:\tmp\glory-target', 'C:\tmp\glory-openapi-target'),
    [string[]]$ExcludeDirs = @(),
    [int]$MaxTotalMB = 15360,
    # Se conserva por compatibilidad con invocaciones existentes; no desactiva
    # las salvaguardas de procesos activos ni la whitelist de C:\tmp.
    [switch]$Force,
    [switch]$AllowCleanupWhileBuildActive
)

$ErrorActionPreference = 'Stop'
$script:ProcessInspectionFailed = $false

function Normalize-Path {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $null
    }

    try {
        return [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    } catch {
        return $Path.TrimEnd('\')
    }
}

function Test-IsExcludedPath {
    param(
        [string]$Path,
        [string[]]$ExcludedPrefixes
    )

    $normalizedPath = Normalize-Path -Path $Path
    if (-not $normalizedPath) {
        return $false
    }

    foreach ($excluded in $ExcludedPrefixes) {
        if (-not $excluded) {
            continue
        }

        if ($normalizedPath.Equals($excluded, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }

        if ($normalizedPath.StartsWith("$excluded\", [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }

    return $false
}

function Get-DirectorySizeMB {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return 0
    }

    [Int64]$totalBytes = 0
    Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue | ForEach-Object {
        $totalBytes += $_.Length
    }
    return [math]::Round($totalBytes / 1MB, 2)
}

function Get-CombinedDirectorySizeMB {
    param([string[]]$Paths)

    [double]$totalSize = 0
    foreach ($path in $Paths) {
        $totalSize += Get-DirectorySizeMB -Path $path
    }
    return [math]::Round($totalSize, 2)
}

function Test-CleanupProcessSafety {
    param([switch]$AllowBuildActive)

    $rustProcesses = @(Get-RustProcessSnapshot)
    if ($script:ProcessInspectionFailed) {
        Write-Host '[cargo-clean] no se pudo inspeccionar procesos Rust; se pospone esta fase'
        return $false
    }

    if ($rustProcesses.Count -eq 0) {
        return $true
    }

    if (-not $AllowBuildActive) {
        Write-Host '[cargo-clean] build Rust activo; se requiere -AllowCleanupWhileBuildActive y marcadores'
        return $false
    }

    if (-not (Test-OnlyMarkedRustBuildsActive -Processes $rustProcesses -MarkerPids $activeMarkerPids)) {
        Write-Host '[cargo-clean] build Rust sin marcador verificable; se pospone esta fase'
        return $false
    }

    return $true
}

function Remove-MatchingDirectories {
    param(
        [string]$Path,
        [string[]]$Names,
        [string[]]$ExcludedPrefixes
    )

    foreach ($name in $Names) {
        Get-ChildItem -LiteralPath $Path -Recurse -Force -Directory -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Name -eq $name -and -not (Test-IsExcludedPath -Path $_.FullName -ExcludedPrefixes $ExcludedPrefixes)
            } |
            Sort-Object FullName -Descending |
            ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction SilentlyContinue
            }
    }
}

function Get-ActiveMarkerMetadata {
    param([string[]]$BasePaths)

    $metadataItems = @()
    foreach ($basePath in $BasePaths) {
        if (-not (Test-Path $basePath)) {
            continue
        }

        $candidates = @(
            (Get-Item -LiteralPath $basePath -ErrorAction SilentlyContinue),
            (Get-ChildItem -LiteralPath $basePath -Force -Directory -ErrorAction SilentlyContinue)
        ) | Where-Object { $_ }
        foreach ($candidate in $candidates) {
            $markers = Get-ChildItem -LiteralPath $candidate.FullName -Force -File -Filter '.glory-cargo-active-*.json' -ErrorAction SilentlyContinue
            foreach ($marker in $markers) {
                try {
                    $metadata = Get-Content -LiteralPath $marker.FullName -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
                    $processId = [int]$metadata.pid
                    $process = Get-Process -Id $processId -ErrorAction Stop
                    $createdAt = [DateTimeOffset]::Parse([string]$metadata.createdAt)
                    $processStartedAt = [DateTimeOffset]$process.StartTime.ToUniversalTime()
                    if ($processStartedAt -gt $createdAt.AddSeconds(5)) {
                        throw "PID reutilizado para el marcador $($marker.Name)"
                    }
                    $metadataItems += [pscustomobject]@{
                        Path = $candidate.FullName
                        Pid = $processId
                    }
                } catch {
                    Remove-Item -LiteralPath $marker.FullName -Force -ErrorAction SilentlyContinue
                }
            }
        }
    }
    return @($metadataItems)
}

function Test-ProcessHasMarkerAncestor {
    param(
        [int]$ProcessId,
        [int[]]$MarkerPids
    )

    $currentId = $ProcessId
    for ($depth = 0; $depth -lt 32; $depth++) {
        if ($MarkerPids -contains $currentId) {
            return $true
        }
        $processInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $currentId" -ErrorAction SilentlyContinue
        if (-not $processInfo -or [int]$processInfo.ParentProcessId -eq 0 -or [int]$processInfo.ParentProcessId -eq $currentId) {
            return $false
        }
        $currentId = [int]$processInfo.ParentProcessId
    }
    return $false
}

function Get-RustProcessSnapshot {
    try {
        return @(Get-CimInstance Win32_Process -Filter "Name = 'cargo.exe' OR Name = 'rustc.exe'" -ErrorAction Stop)
    } catch {
        $script:ProcessInspectionFailed = $true
        return @()
    }
}

function Test-OnlyMarkedRustBuildsActive {
    param(
        [object[]]$Processes,
        [int[]]$MarkerPids
    )

    foreach ($processInfo in $Processes) {
        if (-not (Test-ProcessHasMarkerAncestor -ProcessId ([int]$processInfo.ProcessId) -MarkerPids $MarkerPids)) {
            return $false
        }
    }
    return $true
}

$normalizedTargetDirs = @($TargetDirs | ForEach-Object { Normalize-Path -Path $_ } | Where-Object { $_ })
$allowedTargetRoots = @(
    (Normalize-Path -Path 'C:\tmp\glory-target'),
    (Normalize-Path -Path 'C:\tmp\glory-openapi-target')
)
foreach ($normalizedTargetDir in $normalizedTargetDirs) {
    $isAllowedTarget = @($allowedTargetRoots | Where-Object {
        $normalizedTargetDir.Equals($_, [System.StringComparison]::OrdinalIgnoreCase) -or
        $normalizedTargetDir.StartsWith("$_\", [System.StringComparison]::OrdinalIgnoreCase)
    }).Count -gt 0
    if (-not $isAllowedTarget) {
        throw "TargetDir no permitido; solo se aceptan targets conocidos bajo C:\tmp: $normalizedTargetDir"
    }
}

$activeMarkerMetadata = @(Get-ActiveMarkerMetadata -BasePaths $normalizedTargetDirs)
$discoveredActiveDirs = @($activeMarkerMetadata | ForEach-Object { $_.Path })
$activeMarkerPids = @($activeMarkerMetadata | ForEach-Object { [int]$_.Pid })
$normalizedExcludeDirs = @(
    @($ExcludeDirs) + $discoveredActiveDirs |
        ForEach-Object { Normalize-Path -Path $_ } |
        Where-Object { $_ } |
        Sort-Object -Unique
)

$combinedInitialSize = Get-CombinedDirectorySizeMB -Paths $normalizedTargetDirs
if ($combinedInitialSize -le $MaxTotalMB) {
    Write-Host "[cargo-clean] targets C:\tmp OK ($combinedInitialSize MB / $MaxTotalMB MB)"
    exit 0
}

Write-Host "[cargo-clean] targets C:\tmp sobre límite ($combinedInitialSize MB / $MaxTotalMB MB); inicia poda conocida"

$cleanupPhases = @(
    @{ Names = @('incremental'); Label = 'incremental' },
    @{ Names = @('.fingerprint', 'build'); Label = 'metadatos' },
    @{ Names = @('deps'); Label = 'dependencias' }
)

foreach ($phase in $cleanupPhases) {
    foreach ($targetDir in $normalizedTargetDirs) {
        if (-not (Test-Path $targetDir)) {
            continue
        }

        if (-not (Test-CleanupProcessSafety -AllowBuildActive:$AllowCleanupWhileBuildActive)) {
            continue
        }

        Write-Host "[cargo-clean] fase $($phase.Label): $targetDir"
        Remove-MatchingDirectories -Path $targetDir -Names $phase.Names -ExcludedPrefixes $normalizedExcludeDirs
    }

    $combinedSize = Get-CombinedDirectorySizeMB -Paths $normalizedTargetDirs
    Write-Host "[cargo-clean] total tras fase $($phase.Label): $combinedSize MB / $MaxTotalMB MB"
    if ($combinedSize -le $MaxTotalMB) {
        Write-Host '[cargo-clean] límite combinado alcanzado'
        exit 0
    }
}

$finalSize = Get-CombinedDirectorySizeMB -Paths $normalizedTargetDirs
Write-Host "[cargo-clean] poda finalizada; total C:\tmp: $finalSize MB / $MaxTotalMB MB"