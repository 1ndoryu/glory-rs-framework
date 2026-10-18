/* Dependencias del frontend: npm install solo cuando cambian package.json o package-lock.json. */
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { frontendDir } from './contexto.mjs';
import { resolveSpawnInvocation } from './plataforma.mjs';

const frontendPackageJson = resolve(frontendDir, 'package.json');
const frontendPackageLock = resolve(frontendDir, 'package-lock.json');
const frontendNodeModules = resolve(frontendDir, 'node_modules');
const frontendInstallMarker = resolve(frontendNodeModules, '.glory-dev-install.json');

function hashFile(path) {
    if (!existsSync(path)) {
        return null;
    }

    return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function frontendDependencyFingerprint() {
    return {
        packageJson: hashFile(frontendPackageJson),
        packageLock: hashFile(frontendPackageLock),
        installer: 'npm install --no-audit --no-fund',
    };
}

function readFrontendInstallMarker() {
    if (!existsSync(frontendInstallMarker)) {
        return null;
    }

    try {
        return JSON.parse(readFileSync(frontendInstallMarker, 'utf8'));
    } catch {
        return null;
    }
}

function frontendInstallMatches(marker, fingerprint) {
    return marker
        && marker.packageJson === fingerprint.packageJson
        && marker.packageLock === fingerprint.packageLock
        && marker.installer === fingerprint.installer;
}

function runNpmInFrontend(args) {
    const invocation = resolveSpawnInvocation('npm', args);
    return spawnSync(invocation.executable, invocation.args, {
        cwd: frontendDir,
        env: process.env,
        stdio: 'inherit',
    });
}

export function ensureFrontendDependencies() {
    if (process.env.GLORY_DEV_SKIP_FRONTEND_INSTALL === '1') {
        console.warn('[glory-dev] GLORY_DEV_SKIP_FRONTEND_INSTALL=1; no se sincronizan dependencias frontend.');
        return;
    }

    const fingerprint = frontendDependencyFingerprint();
    const marker = readFrontendInstallMarker();
    if (existsSync(frontendNodeModules) && frontendInstallMatches(marker, fingerprint)) {
        return;
    }

    console.log('[glory-dev] Dependencias frontend desfasadas o sin instalar; ejecutando npm install...');
    const installResult = runNpmInFrontend(['install', '--no-audit', '--no-fund']);
    if (installResult.error) {
        console.error(`[glory-dev] Error ejecutando npm install: ${installResult.error.message}`);
        process.exit(1);
    }

    if (installResult.status !== 0) {
        console.error('[glory-dev] No se pudieron sincronizar las dependencias frontend.');
        process.exit(installResult.status ?? 1);
    }

    const installedFingerprint = frontendDependencyFingerprint();
    writeFileSync(frontendInstallMarker, JSON.stringify({
        ...installedFingerprint,
        updatedAt: new Date().toISOString(),
    }, null, 2));
    console.log('[glory-dev] Dependencias frontend sincronizadas.');
}
