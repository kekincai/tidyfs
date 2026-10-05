#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {render} from 'ink';
import {App} from './app.js';
import {EngineClient} from './engine/client.js';
import {createLogger} from './lib/logger.js';

/** 引擎查找顺序：环境变量 TIDYFS_ENGINE（双击 exe 启动时由 exe 设置）→ 仓库里的 release/debug 构建 → 同目录。 */
function findEngine(): string | null {
	const fromEnv = process.env['TIDYFS_ENGINE'];
	if (fromEnv && fs.existsSync(fromEnv)) return fromEnv;

	const exe = process.platform === 'win32' ? 'tidyfs.exe' : 'tidyfs';
	const uiDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
	const candidates = [
		path.join(uiDir, '..', 'target', 'release', exe),
		path.join(uiDir, '..', 'target', 'debug', exe),
		path.join(uiDir, '..', exe),
	];
	// 同时存在 release 和 debug 时用最新编译的那个，避免开发时用到旧版本。
	const found = candidates
		.map(candidate => ({candidate, stat: fs.statSync(candidate, {throwIfNoEntry: false})}))
		.filter(entry => entry.stat?.isFile())
		.sort((a, b) => b.stat!.mtimeMs - a.stat!.mtimeMs);
	return found[0]?.candidate ?? null;
}

const enginePath = findEngine();
const log = createLogger(enginePath);
if (!enginePath) {
	console.error('找不到 tidyfs 引擎。请先在仓库根目录执行 cargo build --release，或设置环境变量 TIDYFS_ENGINE。');
	process.exit(1);
}
if (!process.stdin.isTTY) {
	console.error('交互界面需要在终端里运行。脚本中请直接使用 tidyfs empty / tidyfs flatten 命令。');
	process.exit(1);
}

process.on('uncaughtException', error => {
	log.fatal({err: error}, 'uncaught exception');
	log.flush();
});

const engine = new EngineClient(enginePath, log);
const instance = render(<App engine={engine} log={log} />, {alternateScreen: true, exitOnCtrlC: true});
await instance.waitUntilExit();
engine.close();
log.flush();
