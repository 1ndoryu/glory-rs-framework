/* Adaptación por plataforma: nombres de ejecutables y lanzamiento seguro en
 * Windows (los .cmd/.bat necesitan cmd.exe y comillas propias). */
import { relative, resolve, sep } from 'node:path';

export const isWin = process.platform === 'win32';

/* En Windows los ejecutables llevan extensión explícita; en el resto, el nombre tal cual. */
export function commandName(cmd) {
    if (!isWin) {
        return cmd;
    }

    if (cmd === 'npm') {
        return 'npm.cmd';
    }
    if (cmd === 'cargo') {
        return 'cargo.exe';
    }
    if (cmd === 'git') {
        return 'git.exe';
    }
    if (cmd === 'powershell') {
        return 'powershell.exe';
    }
    if (cmd === 'psql') {
        return 'psql.exe';
    }
    return cmd;
}

/* Escapa un argumento para la línea de cmd.exe (comillas y barras finales). */
export function quoteWindowsArg(arg) {
    if (!/[\s"]/u.test(arg)) {
        return arg;
    }

    return `"${arg
        .replace(/(\\*)"/g, '$1$1\\"')
        .replace(/(\\+)$/g, '$1$1')}"`;
}

/* Devuelve ejecutable y argumentos listos para spawn; los .cmd/.bat van por cmd.exe. */
export function resolveSpawnInvocation(cmd, args) {
    const executable = commandName(cmd);
    if (isWin && /\.(cmd|bat)$/i.test(executable)) {
        const shellCommand = [executable, ...args].map((value) => quoteWindowsArg(value)).join(' ');
        return {
            executable: process.env.ComSpec || 'cmd.exe',
            args: ['/d', '/s', '/c', shellCommand],
        };
    }

    return { executable, args };
}

/* True si `candidate` está dentro de `root` (o es el propio root). */
export function isPathInside(root, candidate) {
    const relativePath = relative(resolve(root), resolve(candidate));
    return relativePath === ''
        || (relativePath !== '..'
            && !relativePath.startsWith(`..${sep}`)
            && !relativePath.includes(`${sep}..${sep}`));
}
