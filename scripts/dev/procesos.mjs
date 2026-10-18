/* Procesos hijos del launcher y su cierre ordenado (árbol completo en Windows). */
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, unlinkSync } from 'node:fs';
import { isWin, resolveSpawnInvocation } from './plataforma.mjs';

const hijos = [];
/* Marcador de actividad de cargo de esta instancia; lo fija el orquestador. */
let marcadorActividad = null;

export function fijarMarcadorActividad(ruta) {
    marcadorActividad = ruta;
}

export function spawnProc(label, cmd, args, options) {
    const invocation = resolveSpawnInvocation(cmd, args);
    const proc = spawn(invocation.executable, invocation.args, {
        stdio: 'inherit',
        ...options,
    });
    proc.on('error', (err) => console.error(`[${label}] Error: ${err.message}`));
    proc.on('exit', (code) => {
        console.log(`[${label}] Proceso terminado con codigo ${code}`);
        cleanup();
    });
    hijos.push(proc);
    return proc;
}

/* [038A-1] cleanup(): al cerrar dev, matar el ARBOL completo de procesos en
 * Windows. child.kill() solo mata el proceso directo (cargo) y deja huerfano
 * a su nieto (glory-backend.exe), que bloquea el binario en el proximo arranque
 * (Acceso denegado os error 5) y bloquea la poda de targets ("build Rust activo"
 * sin marcador verificable). taskkill /T /F derriba todo el arbol de procesos. */
function killProcessTree(pid) {
    if (!isWin || !pid) {
        return;
    }
    try {
        spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore', timeout: 10000 });
    } catch {
        /* si taskkill falla, el proceso ya no existe; no hay mas que hacer */
    }
}

function cleanup() {
    for (const child of hijos) {
        if (child.killed) {
            continue;
        }
        if (isWin) {
            killProcessTree(child.pid);
        } else {
            child.kill();
        }
    }
    if (marcadorActividad && existsSync(marcadorActividad)) {
        try {
            unlinkSync(marcadorActividad);
        } catch {
            console.warn(`[glory-dev] No se pudo retirar el marcador ${marcadorActividad}`);
        }
    }
}

/* Cierre al recibir señales o al salir del proceso. */
export function registrarSalida() {
    process.on('SIGINT', cleanup);
    process.on('SIGTERM', cleanup);
    process.once('exit', cleanup);
}
