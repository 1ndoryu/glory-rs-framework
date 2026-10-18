/* Base de datos local: crea la BD de la rama, aplica migraciones sqlx y, si hay drift, resetea el schema public. */
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { projectRoot } from './contexto.mjs';
import { commandName, isWin } from './plataforma.mjs';

export function findPsql() {
    const command = spawnSync(commandName('psql'), ['--version'], { encoding: 'utf8' });
    if (command.status === 0) {
        return commandName('psql');
    }

    if (!isWin) {
        return null;
    }

    const postgresRoot = 'C:\\Program Files\\PostgreSQL';
    if (!existsSync(postgresRoot)) {
        return null;
    }

    const versions = readdirSync(postgresRoot).sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
    for (const version of versions) {
        const candidate = resolve(postgresRoot, version, 'bin', 'psql.exe');
        if (existsSync(candidate)) {
            return candidate;
        }
    }
    return null;
}

/* Solo identificadores alfanuméricos: el nombre de BD nunca entra sin escapar. */
export function quoteSqlIdentifier(value) {
    if (!/^[a-zA-Z0-9_]+$/.test(value)) {
        throw new Error(`Nombre de BD inseguro: ${value}`);
    }
    return `"${value.replaceAll('"', '""')}"`;
}

function runPsql(psql, url, sql) {
    const parsed = new URL(url);
    const env = {
        ...process.env,
        PGPASSWORD: decodeURIComponent(parsed.password),
    };
    const args = [
        '-h',
        parsed.hostname,
        '-p',
        parsed.port || '5432',
        '-U',
        decodeURIComponent(parsed.username),
        '-d',
        'postgres',
        '-tAc',
        sql,
    ];
    return spawnSync(psql, args, { cwd: projectRoot, env, encoding: 'utf8' });
}

function resetPublicSchema(psql, databaseUrl) {
    const parsed = new URL(databaseUrl);
    const env = {
        ...process.env,
        PGPASSWORD: decodeURIComponent(parsed.password),
    };
    const args = [
        '-h',
        parsed.hostname,
        '-p',
        parsed.port || '5432',
        '-U',
        decodeURIComponent(parsed.username),
        '-d',
        parsed.pathname.slice(1),
        '-c',
        'DROP SCHEMA public CASCADE; CREATE SCHEMA public; GRANT ALL ON SCHEMA public TO CURRENT_USER;',
    ];
    return spawnSync(psql, args, { cwd: projectRoot, env, encoding: 'utf8' });
}

/* CARGO_TARGET_DIR va explícito para que sqlx use el mismo target que el backend. */
function runSqlxMigrations(databaseUrl, cargoTargetDir) {
    const env = {
        ...process.env,
        DATABASE_URL: databaseUrl,
        CARGO_TARGET_DIR: cargoTargetDir,
    };
    return spawnSync(commandName('cargo'), ['sqlx', 'migrate', 'run'], { cwd: projectRoot, env, encoding: 'utf8' });
}

export function ensureMigrationsAreCompatible(databaseUrl, dbName, cargoTargetDir) {
    const firstRun = runSqlxMigrations(databaseUrl, cargoTargetDir);
    if (firstRun.status === 0) {
        return;
    }

    const output = `${firstRun.stdout}\n${firstRun.stderr}`;
    /* [028A-1] Ademas del historial faltante/modificado, la BD local de desarrollo
     * puede quedar con objetos creados fuera de sqlx (prototipos manuales, agentes
     * en paralelo, restauraciones parciales): `sqlx migrate run` falla con
     * "relation ... already exists" / "la relacion ... ya existe" (SQLSTATE 42P07)
     * porque la tabla existe sin fila en _sqlx_migrations. Ese drift tambien es
     * una incompatibilidad de migracion y se cura con el mismo reset del schema
     * public local. */
    if (!/VersionMissing|VersionMismatch|previously applied but has been modified|already exists|ya existe/.test(output)) {
        process.stdout.write(firstRun.stdout);
        process.stderr.write(firstRun.stderr);
        console.error('[glory-dev] No se pudieron aplicar las migraciones locales.');
        process.exit(firstRun.status ?? 1);
    }

    const psql = findPsql();
    if (!psql) {
        process.stderr.write(firstRun.stderr);
        console.error('[glory-dev] La BD local tiene migraciones incompatibles, pero psql no esta disponible para resetearla.');
        process.exit(firstRun.status ?? 1);
    }

    console.warn(`[glory-dev] Migraciones incompatibles o drift en ${dbName}; reseteando schema public de desarrollo.`);
    const resetResult = resetPublicSchema(psql, databaseUrl);
    if (resetResult.status !== 0) {
        process.stderr.write(resetResult.stderr);
        console.error('[glory-dev] No se pudo resetear la BD local de desarrollo.');
        process.exit(resetResult.status ?? 1);
    }

    const secondRun = runSqlxMigrations(databaseUrl, cargoTargetDir);
    if (secondRun.status !== 0) {
        process.stdout.write(secondRun.stdout);
        process.stderr.write(secondRun.stderr);
        console.error('[glory-dev] Las migraciones siguen fallando tras resetear la BD local.');
        process.exit(secondRun.status ?? 1);
    }
}

export function ensureDatabaseExists(databaseUrl, dbName) {
    const psql = findPsql();
    if (!psql) {
        console.warn('[glory-dev] psql no esta disponible; si la BD no existe, el backend fallara al conectar.');
        return;
    }

    const existsResult = runPsql(psql, databaseUrl, `SELECT 1 FROM pg_database WHERE datname = '${dbName}'`);
    if (existsResult.status !== 0) {
        console.warn('[glory-dev] No se pudo verificar la BD local:', existsResult.stderr.trim());
        return;
    }

    if (existsResult.stdout.trim() === '1') {
        return;
    }

    const createResult = runPsql(psql, databaseUrl, `CREATE DATABASE ${quoteSqlIdentifier(dbName)}`);
    if (createResult.status !== 0) {
        console.warn('[glory-dev] No se pudo crear la BD local:', createResult.stderr.trim());
        return;
    }

    console.log(`[glory-dev] BD local creada: ${dbName}`);
}
