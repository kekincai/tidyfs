// 端到端界面测试：真实启动引擎，在临时目录上走完“添加路径 → 扫描 → 确认 → 执行 → 完成”。
// 运行前需要先 cargo build。设置 TIDYFS_SHOW_FRAMES=1 可以打印每一步的界面。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {after, before, test} from 'node:test';
import {fileURLToPath} from 'node:url';
import {render} from 'ink-testing-library';
import pino from 'pino';
import {App} from '../app.js';
import {EngineClient} from '../engine/client.js';

const exe = process.platform === 'win32' ? 'tidyfs.exe' : 'tidyfs';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const engines = ['debug', 'release']
	.map(profile => path.join(repo, 'target', profile, exe))
	.filter(candidate => fs.existsSync(candidate))
	.sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
const enginePath = process.env['TIDYFS_ENGINE'] ?? engines[0];

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const show = (label: string, frame: string | undefined) => {
	if (process.env['TIDYFS_SHOW_FRAMES']) console.log(`\n===== ${label} =====\n${frame ?? ''}`);
};

async function waitFor(read: () => string | undefined, text: string, timeout = 10_000): Promise<string> {
	const started = Date.now();
	while (Date.now() - started < timeout) {
		const frame = read() ?? '';
		if (frame.includes(text)) {
			// 画面出来后，新页面的按键监听在下一轮 effect 里才挂上。
			await sleep(100);
			return frame;
		}
		await sleep(30);
	}
	throw new Error(`等待“${text}”超时，最后一帧：\n${read()}`);
}

let root: string;
before(() => {
	root = fs.mkdtempSync(path.join(os.tmpdir(), 'tidyfs-ui-'));
	fs.mkdirSync(path.join(root, 'a', 'b', 'c'), {recursive: true});
	fs.mkdirSync(path.join(root, 'keep'), {recursive: true});
	fs.writeFileSync(path.join(root, 'keep', 'file.txt'), 'x');
	fs.mkdirSync(path.join(root, 'only-noise'));
	fs.writeFileSync(path.join(root, 'only-noise', 'Thumbs.db'), 'x');
	process.env['TIDYFS_JOURNAL_DIR'] = path.join(root, '..', path.basename(root) + '-journals');
});
after(() => fs.rmSync(root, {recursive: true, force: true}));

test('完整流程：清理空文件夹', {skip: enginePath ? false : '没有找到引擎，先运行 cargo build'}, async () => {
	const engine = new EngineClient(enginePath!, pino({level: 'silent'}));
	const ui = render(<App engine={engine} log={pino({level: 'silent'})} />);
	try {
		show('启动', await waitFor(ui.lastFrame, '选择位置'));

		ui.stdin.write('p');
		await waitFor(ui.lastFrame, '路径');
		ui.stdin.write(root);
		await sleep(50);
		ui.stdin.write('\r');
		show('添加路径后', await waitFor(ui.lastFrame, path.basename(root)));

		ui.stdin.write('\r');
		const review = await waitFor(ui.lastFrame, '将处理');
		show('扫描结果', review);
		assert.match(review, /将处理 4 个空文件夹/);

		ui.stdin.write('\r');
		show('确认', await waitFor(ui.lastFrame, '按 y 开始'));
		ui.stdin.write('y');

		const summary = await waitFor(ui.lastFrame, '全部完成');
		show('完成', summary);
		assert.ok(!fs.existsSync(path.join(root, 'a')));
		assert.ok(!fs.existsSync(path.join(root, 'only-noise')));
		assert.ok(fs.existsSync(path.join(root, 'keep', 'file.txt')));
	} finally {
		ui.unmount();
		engine.close();
	}
});
