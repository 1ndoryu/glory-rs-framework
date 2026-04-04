#!/usr/bin/env node
/* [044A-9] Comando de desarrollo unificado para proyectos glory-rs.
 * Ejecuta backend Rust (cargo run) y frontend Vite (npm run dev) en paralelo.
 * Reside en glory-rs para ser reutilizable en cualquier proyecto.
 *
 * Uso desde la raiz del proyecto:
 *   node glory-rs/scripts/dev.mjs
 *   o via npm script: npm run dev
 *
 * Ambos procesos se lanzan con stdio heredado (output directo a terminal).
 * Ctrl+C mata ambos procesos limpiamente. */

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const cwd = process.cwd();
const cargoToml = resolve(cwd, 'Cargo.toml');
const frontendDir = resolve(cwd, 'frontend');

if (!existsSync(cargoToml)) {
    console.error('[glory-dev] No se encontro Cargo.toml en', cwd);
    process.exit(1);
}
if (!existsSync(frontendDir)) {
    console.error('[glory-dev] No se encontro frontend/ en', cwd);
    process.exit(1);
}

const isWin = process.platform === 'win32';
/* En Windows, spawn no resuelve ejecutables en PATH sin shell.
 * Usar shell: true es seguro porque los args van como array (no concatenados). */
const spawnOpts = isWin ? { shell: true } : {};
const children = [];

function spawnProc(label, cmd, args, options) {
    const proc = spawn(cmd, args, {
        stdio: 'inherit',
        ...spawnOpts,
        ...options,
    });
    proc.on('error', (err) => console.error(`[${label}] Error: ${err.message}`));
    proc.on('exit', (code) => {
        console.log(`[${label}] Proceso terminado con codigo ${code}`);
        cleanup();
    });
    children.push(proc);
    return proc;
}

function cleanup() {
    for (const child of children) {
        if (!child.killed) {
            child.kill();
        }
    }
}

process.on('SIGINT', cleanup);
process.on('SIGTERM', cleanup);

/* Detecta el nombre del binario principal leyendo Cargo.toml [package] name */
function detectBinName() {
    const toml = readFileSync(cargoToml, 'utf8');
    const match = toml.match(/^name\s*=\s*"([^"]+)"/m);
    return match ? match[1] : null;
}

const binName = detectBinName();
if (!binName) {
    console.error('[glory-dev] No se pudo detectar el nombre del binario en Cargo.toml');
    process.exit(1);
}

console.log(`[glory-dev] Iniciando backend (cargo run --bin ${binName}) y frontend (vite)...\n`);

spawnProc('backend', 'cargo', ['run', '--bin', binName], { cwd });
spawnProc('frontend', 'npm', ['run', 'dev'], { cwd: frontendDir });
