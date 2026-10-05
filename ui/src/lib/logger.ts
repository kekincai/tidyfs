import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import pino, {type Logger} from 'pino';

const MAX_LOG_BYTES = 5 * 1024 * 1024;

/**
 * 和引擎一致：优先写在部署目录（TIDYFS_HOME 或引擎 exe 所在目录）的 logs 下，
 * 不可写时退回 %LOCALAPPDATA%\tidyfs\logs。绝不写进被处理的文件夹。
 */
export function logDir(enginePath: string | null): string {
	const home = process.env['TIDYFS_HOME'] ?? (enginePath ? path.dirname(enginePath) : null);
	if (home) {
		const dir = path.join(home, 'logs');
		try {
			fs.mkdirSync(dir, {recursive: true});
			fs.accessSync(dir, fs.constants.W_OK);
			return dir;
		} catch {
			// 部署目录不可写，用下面的备用位置。
		}
	}
	return path.join(process.env['LOCALAPPDATA'] ?? os.tmpdir(), 'tidyfs', 'logs');
}

export function createLogger(enginePath: string | null): Logger {
	const dir = logDir(enginePath);
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
