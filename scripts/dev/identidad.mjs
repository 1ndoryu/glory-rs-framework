/* Identidad del proyecto y de la rama: nombres, BD por rama y lectura de .env y Cargo.toml. */
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { cargoToml, projectRoot } from './contexto.mjs';
import { commandName } from './plataforma.mjs';

function runGit(args) {
    const result = spawnSync(commandName('git'), args, { cwd: projectRoot, encoding: 'utf8' });
    return result.status === 0 ? result.stdout.trim() : '';
}

export function detectBranch() {
    return runGit(['branch', '--show-current']) || runGit(['rev-parse', '--short', 'HEAD']) || 'local';
}

export function slugifyBranchName(branch) {
    const slug = branch
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '_')
        .replace(/^_+|_+$/g, '');
    return slug || 'local';
}

export function detectPackageName() {
    const toml = readFileSync(cargoToml, 'utf8');
    const match = toml.match(/^name\s*=\s*"([^"]+)"/m);
    return (match ? match[1] : 'glory').replace(/-/g, '_');
}

export function detectBinName() {
    const toml = readFileSync(cargoToml, 'utf8');
    const match = toml.match(/^name\s*=\s*"([^"]+)"/m);
    return match ? match[1] : null;
}

/* Lee pares CLAVE=valor de un .env (sin comentarios; comillas simples o dobles se quitan). */
export function parseEnvFile(path) {
    if (!existsSync(path)) {
        return new Map();
    }

    const entries = new Map();
    for (const rawLine of readFileSync(path, 'utf8').split(/\r?\n/)) {
        const line = rawLine.trim();
        if (!line || line.startsWith('#') || !line.includes('=')) {
            continue;
        }

        const separator = line.indexOf('=');
        const key = line.slice(0, separator).trim();
        let value = line.slice(separator + 1).trim();
        if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
            value = value.slice(1, -1);
        }
        entries.set(key, value);
    }
    return entries;
}

/* BD de la rama: GLORY_DEV_DB_NAME manda; main/master usan el nombre del paquete. */
export function databaseNameForBranch(branch) {
    if (process.env.GLORY_DEV_DB_NAME) {
        return process.env.GLORY_DEV_DB_NAME;
    }
    const pkgName = detectPackageName();
    const isDefault = branch === 'main' || branch === 'master';
    return isDefault ? pkgName : `${pkgName}_${slugifyBranchName(branch)}`;
}

/* URL de la BD: plantilla con {db} si existe; si no, DATABASE_URL con el nombre cambiado. */
export function databaseUrlForName(envValues, dbName) {
    const template = process.env.GLORY_DEV_DATABASE_URL_TEMPLATE || envValues.get('GLORY_DEV_DATABASE_URL_TEMPLATE');
    if (template) {
        return template.replaceAll('{db}', dbName);
    }

    const baseUrl = process.env.DATABASE_URL || envValues.get('DATABASE_URL') || 'postgres://postgres:root@localhost:5432/postgres';
    const parsed = new URL(baseUrl);
    parsed.pathname = `/${dbName}`;
    return parsed.toString();
}
