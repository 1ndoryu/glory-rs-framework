#!/usr/bin/env node
/* [085A-2] Launcher compartido de desarrollo para proyectos glory-rs.
 * Vive en glory-rs para que el cambio de ramas/proyectos no duplique tooling local.
 * Centraliza: BD PostgreSQL por rama, target Cargo por rama, sccache, poda de targets
 * y sincronizacion de dependencias frontend antes de arrancar Vite.
 * Este archivo solo orquesta el orden de arranque; cada responsabilidad vive en scripts/dev/. */

import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { cargoToml, cargoTargetBase, devArgs, envPath, frontendDir, projectRoot } from './dev/contexto.mjs';
import { databaseNameForBranch, databaseUrlForName, detectBinName, detectBranch, detectPackageName, parseEnvFile, slugifyBranchName } from './dev/identidad.mjs';
import { ensureFrontendDependencies } from './dev/frontend.mjs';
import { ensureDatabaseExists, ensureMigrationsAreCompatible } from './dev/base-datos.mjs';
import { createCargoActivityMarker, killStaleProjectProcesses, limpiarTargets, resolveRustcWrapper, spawnCargoTargetWatcher } from './dev/cargo.mjs';
import { fijarMarcadorActividad, registrarSalida, spawnProc } from './dev/procesos.mjs';
import { printForeignTakeovers } from './dev/tomas.mjs';

if (!existsSync(cargoToml)) {
    console.error('[glory-dev] No se encontro Cargo.toml en', projectRoot);
    process.exit(1);
}

if (!existsSync(frontendDir)) {
    console.error('[glory-dev] No se encontro frontend/ en', projectRoot);
    process.exit(1);
}

const syncFrontendOnly = devArgs.includes('--sync-frontend');
const skipMigrations = process.env.GLORY_DEV_SKIP_MIGRATIONS === '1' || devArgs.includes('--skip-migrations');

registrarSalida();

const envValues = parseEnvFile(envPath);
const branch = detectBranch();
const cargoTargetDir = process.env.GLORY_CARGO_TARGET_DIR || resolve(cargoTargetBase, `${detectPackageName()}_${slugifyBranchName(branch)}`);
const cargoActivityMarker = join(cargoTargetDir, `.glory-cargo-active-${process.pid}.json`);
fijarMarcadorActividad(cargoActivityMarker);
const dbName = databaseNameForBranch(branch);
if (!/^[a-zA-Z0-9_]+$/.test(dbName)) {
    console.error(`[glory-dev] Nombre de BD inseguro: ${dbName}`);
    process.exit(1);
}
const databaseUrl = databaseUrlForName(envValues, dbName);
const binName = detectBinName();

if (!binName) {
    console.error('[glory-dev] No se pudo detectar el nombre del binario en Cargo.toml');
    process.exit(1);
}

if (devArgs.includes('--print-db')) {
    console.log(databaseUrl);
    process.exit(0);
}

if (syncFrontendOnly) {
    ensureFrontendDependencies();
    process.exit(0);
}

ensureFrontendDependencies();

createCargoActivityMarker(cargoTargetDir, cargoActivityMarker);

ensureDatabaseExists(databaseUrl, dbName);
if (skipMigrations) {
    console.warn('[glory-dev] --skip-migrations activo; no se aplican migraciones locales.');
} else {
    ensureMigrationsAreCompatible(databaseUrl, dbName, cargoTargetDir);
}

const childEnv = {
    ...process.env,
    DATABASE_URL: databaseUrl,
    CARGO_TARGET_DIR: cargoTargetDir,
    GLORY_DEV_BRANCH: branch,
    GLORY_DEV_DB_NAME: dbName,
    KAMPLES_PG_DBNAME: dbName,
};

const rustcWrapper = resolveRustcWrapper();
if (rustcWrapper) {
    childEnv.RUSTC_WRAPPER = rustcWrapper;
}

printForeignTakeovers();

console.log(`[glory-dev] Proyecto: ${projectRoot}`);
console.log(`[glory-dev] Rama: ${branch}`);
console.log(`[glory-dev] Base local: ${dbName}`);
console.log(`[glory-dev] Cargo target: ${cargoTargetDir}`);
if (rustcWrapper) {
    console.log(`[glory-dev] Rust cache: ${rustcWrapper}`);
}

/* Antes de arrancar: matar huerfanos y podar targets (ver cargo.mjs). */
killStaleProjectProcesses(cargoTargetDir, binName);
limpiarTargets(cargoTargetDir);

console.log(`[glory-dev] Iniciando backend (cargo run --bin ${binName}) y frontend (vite)...\n`);

spawnProc('backend', 'cargo', ['run', '--bin', binName], { cwd: projectRoot, env: childEnv });
spawnProc('frontend', 'npm', ['run', 'dev'], { cwd: frontendDir, env: childEnv });
spawnCargoTargetWatcher(childEnv, cargoTargetDir);
