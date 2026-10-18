/* Aviso de tomas de tarea ajenas: el launcher es compartido, así que lee el registro
 * directamente y de forma defensiva (nunca bloquea el arranque). */
import { readdirSync, readFileSync } from 'node:fs';
import { hostname } from 'node:os';
import { join } from 'node:path';
import { projectRoot } from './contexto.mjs';

/* [028A-17 Fase 2] Visibilidad temprana de tomas de tarea ajenas. El agente que
 * levanta dev debe ver si hay trabajo en paralelo de otros. */
export function printForeignTakeovers() {
    const agent = (process.env.GLORY_AGENT_ID || '').trim() || hostname();
    const registryRoot = join(projectRoot, '.quality-reports', 'task-takeover');
    let names;
    try {
        names = readdirSync(registryRoot);
    } catch {
        return;
    }
    const nowMs = Date.now();
    for (const name of names) {
        if (!name.endsWith('.json')) continue;
        let entry;
        try {
            entry = JSON.parse(readFileSync(join(registryRoot, name), 'utf8'));
        } catch {
            continue;
        }
        if (!entry || typeof entry.takenAtMs !== 'number') continue;
        if (entry.takenBy === agent) continue;
        const staleMs = 6 * 60 * 60 * 1000;
        if (nowMs - entry.takenAtMs > staleMs) continue;
        console.warn(`[task-takeover] EN CURSO por ${entry.takenBy}: ${entry.taskId} (${entry.id}) hasta ${entry.expiresAt}. No la trabajes en paralelo sin coordinar (npm run task:status).`);
    }
}
