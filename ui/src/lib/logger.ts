import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import pino, {type Logger} from 'pino';

const MAX_LOG_BYTES = 5 * 1024 * 1024;

export function logDir(): string {
	const base = process.env['LOCALAPPDATA'] ?? os.tmpdir();
	return path.join(base, 'tidyfs', 'logs');
}

/**
 * 界面日志写到 %LOCALAPPDATA%\tidyfs\logs\ui.log（和引擎日志在同一目录）。
 * 终端被 Ink 占用，所以日志绝不能写到 stdout。级别由 TIDYFS_LOG 控制。
 */
export function createLogger(): Logger {
	const dir = logDir();
	const file = path.join(dir, 'ui.log');
	try {
		fs.mkdirSync(dir, {recursive: true});
		if ((fs.statSync(file, {throwIfNoEntry: false})?.size ?? 0) > MAX_LOG_BYTES) {
			fs.renameSync(file, path.join(dir, 'ui.old.log'));
		}
	} catch {
		// 日志不可用时不影响主流程。
	}

	return pino(
		{
			level: process.env['TIDYFS_LOG'] ?? 'info',
			base: {pid: process.pid},
			timestamp: pino.stdTimeFunctions.isoTime,
		},
		pino.destination({dest: file, sync: false, mkdir: true}),
	);
}
