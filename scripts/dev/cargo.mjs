/* Cargo: target por rama, sccache, vigilancia y poda de targets, y limpieza de huérfanos del backend. */
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { cargoCleanIntervalSeconds, cargoTargetBase, cargoTargetMaxMb, frameworkScriptDir, projectRoot } from './contexto.mjs';
import { commandName, isPathInside, isWin } from './plataforma.mjs';
import { spawnProc } from './procesos.mjs';

/* La limpieza solo puede recibir los dos roots Cargo conocidos y sus
 * descendientes. Un CARGO_TARGET_DIR_BASE personalizado fuera de ellos sigue
 * siendo válido para Cargo, pero queda deliberadamente fuera del limpiador. */
const knownCargoCleanupRoots = isWin
    ? ['C:\\tmp\\glory-target', 'C:\\tmp\\glory-openapi-target']
    : [];
export const cargoCleanupTargets = isWin
    ? [cargoTargetBase, ...knownCargoCleanupRoots].filter((targetDir, index, targets) =>
        targets.indexOf(targetDir) === index
        && knownCargoCleanupRoots.some((root) => isPathInside(root, targetDir)))
    : [cargoTargetBase];

/* Escribe el marcador que dice a la poda que hay una instancia dev viva en este target. */
export function createCargoActivityMarker(cargoTargetDir, cargoActivityMarker) {
    try {
        mkdirSync(cargoTargetDir, { recursive: true });
        writeFileSync(cargoActivityMarker, JSON.stringify({
            pid: process.pid,
            projectRoot,
            createdAt: new Date().toISOString(),
        }, null, 2));
    } catch (error) {
        console.error(`[glory-dev] No se pudo crear el marcador de actividad: ${error.message}`);
        process.exit(1);
    }
}

/* Usa sccache si está disponible; si no, Cargo compila sin wrapper. */
export function resolveRustcWrapper() {
    if (process.env.RUSTC_WRAPPER) {
        return process.env.RUSTC_WRAPPER;
    }

    if (isWin) {
        const userProfile = process.env.USERPROFILE;
        if (userProfile) {
            const sccachePath = resolve(userProfile, '.cargo', 'bin', 'sccache.exe');
            if (existsSync(sccachePath)) {
                return sccachePath;
            }
        }
    }

    const command = spawnSync('sccache', ['--version'], { encoding: 'utf8', shell: isWin });
    return command.status === 0 ? 'sccache' : null;
}

/* [256A-1c] Watcher de limpieza de target Cargo.
 * El target activo se excluye explícitamente para permitir podar targets
 * antiguos mientras `cargo run` permanece vivo. La limpieza sigue limitada
 * a C:\\tmp\\glory-target y usa la poda progresiva:
 * incremental/ -> .fingerprint+build/ -> deps/. */
export function spawnCargoTargetWatcher(env, activeTargetDir) {
    if (!isWin) {
        return;
    }

    const watcherScript = resolve(frameworkScriptDir, 'watch-cargo-target.ps1');
    if (!existsSync(watcherScript)) {
        return;
    }

    spawnProc(
        'cargo-target-watch',
        'powershell',
        [
            '-ExecutionPolicy',
            'Bypass',
            '-File',
            watcherScript,
            '-TargetDirs',
            ...cargoCleanupTargets,
            '-ExcludeDirs',
            activeTargetDir,
            '-MaxTotalMB',
            cargoTargetMaxMb,
            '-IntervalSeconds',
            cargoCleanIntervalSeconds,
            '-AllowCleanupWhileBuildActive',
        ],
        { cwd: projectRoot, env, stdio: 'ignore' },
    );
}

/* [038A-1] Al arrancar, matar binarios huerfanos de una instancia dev previa
 * cuyo binario apunte al target actual. Si el proceso node murio sin pasar por
 * cleanup() (crash, kill -9, cierre de VS Code), quedan cargo/glory-backend
 * vivos: `cargo run` no puede sobrescribir el .exe (os error 5) y la poda se
 * bloquea por "build Rust activo" sin marcador. Esto se ejecuta ANTES de la
 * pre-limpieza para que la poda corra sin bloqueo.
 * Seguridad: si existe un marcador de actividad valido (otra instancia dev del
 * mismo target corriendo), no se mata nada para no romper esa instancia. */
export function killStaleProjectProcesses(cargoTargetDir, binName) {
    if (!isWin || !binName) {
        return;
    }
    const targetEsc = cargoTargetDir.replace(/'/g, "''");
    const binPathPattern = join(cargoTargetDir, 'debug', `${binName}.exe`).replace(/'/g, "''");
    const psScript = [
        `$target = '${targetEsc}'`,
        `$pattern = '${binPathPattern}'`,
        // Si hay una instancia dev viva del mismo target, su marcador esta
        // activo y apunta a un PID existente: no tocar nada.
        '$markers = Get-ChildItem -LiteralPath $target -Force -File -Filter \'.glory-cargo-active-*.json\' -ErrorAction SilentlyContinue',
        '$aliveMarker = $false',
        'foreach ($m in $markers) {',
        '    try {',
        '        $meta = Get-Content -LiteralPath $m.FullName -Raw | ConvertFrom-Json',
        '        if ($meta.pid -and (Get-Process -Id ([int]$meta.pid) -ErrorAction Stop)) { $aliveMarker = $true; break }',
        '    } catch { /* marcador obsoleto: instancia ya muerta */ }',
        '}',
        'if ($aliveMarker) {',
        '    Write-Output "[glory-dev] Instancia dev activa detectada; no se limpian huerfanos"',
        '} else {',
        '    $found = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {',
        '        $_.CommandLine -and $_.CommandLine -like "*$pattern*"',
        '    }',
        '    if ($found) {',
        '        foreach ($p in $found) {',
        '            Write-Output "[glory-dev] Matando huerfano PID $($p.ProcessId) ($($p.Name))"',
        '            & taskkill /PID $p.ProcessId /T /F 2>&1 | Out-Null',
        '        }',
        '    } else {',
        '        Write-Output "[glory-dev] Sin huerfanos del target actual"',
        '    }',
        '}',
    ].join('; ');
    const result = spawnSync(
        commandName('powershell'),
        ['-NoProfile', '-Command', psScript],
        { cwd: projectRoot, env: process.env, encoding: 'utf8', timeout: 20000 },
    );
    const output = (result.stdout || '').trim();
    if (output && !output.includes('Sin huerfanos') && !output.includes('activa detectada')) {
        process.stdout.write(`${output}\n`);
    }
    if (result.stderr && result.status !== 0) {
        console.warn(`[glory-dev] Aviso limpieza huerfanos: ${result.stderr.trim()}`);
    }
}

/* [256A-1c] Pre-limpieza: si el total de los targets conocidos excede el
 * límite, limpia antes de arrancar. `-Force` solo evita el bloqueo global;
 * `-ExcludeDirs` protege el target activo y los marcadores protegen otras
 * instancias. El script mantiene la whitelist dentro de C:\\tmp. */
export function limpiarTargets(cargoTargetDir) {
    if (!(isWin && cargoCleanupTargets.some((targetDir) => existsSync(targetDir)))) {
        return;
    }
    const cleanScript = resolve(frameworkScriptDir, 'clean-cargo-target.ps1');
    if (!existsSync(cleanScript)) {
        return;
    }
    const cleanResult = spawnSync(
        commandName('powershell'),
        [
            '-ExecutionPolicy', 'Bypass',
            '-File', cleanScript,
            '-TargetDirs', ...cargoCleanupTargets,
            '-ExcludeDirs', cargoTargetDir,
            '-MaxTotalMB', cargoTargetMaxMb,
            '-Force',
        ],
        { cwd: projectRoot, env: process.env, stdio: 'inherit', timeout: 60000 },
    );
    if (cleanResult.status !== 0 && cleanResult.status !== null) {
        console.warn(`[glory-dev] Pre-limpieza de targets fallo con codigo ${cleanResult.status}`);
    }
}
