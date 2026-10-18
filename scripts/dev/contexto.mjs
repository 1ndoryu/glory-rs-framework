/* Rutas y ajustes comunes del launcher, leídos una vez al arrancar. Todo lo
 * demás de scripts/dev/ los importa de aquí para no recalcularlos. */
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { isWin } from './plataforma.mjs';

/* scripts/ (donde viven los .ps1 de limpieza y vigilancia). */
export const frameworkScriptDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const projectRoot = resolve(process.env.GLORY_DEV_PROJECT_ROOT || process.cwd());
export const cargoToml = resolve(projectRoot, 'Cargo.toml');
export const frontendDir = resolve(projectRoot, 'frontend');
export const envPath = resolve(projectRoot, '.env');
// Base de target por proyecto (carpeta del proyecto, no literal): cada consumidor
// del framework obtiene su propio directorio sin tocar este archivo.
const projectTargetName = basename(projectRoot).replace(/[^A-Za-z0-9_-]+/g, '-');
export const cargoTargetBase = process.env.CARGO_TARGET_DIR_BASE || (isWin ? join('C:\\tmp\\glory-target', projectTargetName) : resolve(tmpdir(), 'glory-target'));
export const cargoTargetMaxMb = process.env.GLORY_CARGO_TARGET_MAX_MB || '15360';
export const cargoCleanIntervalSeconds = process.env.GLORY_CARGO_CLEAN_INTERVAL_SECONDS || '120';
export const devArgs = process.argv.slice(2);
