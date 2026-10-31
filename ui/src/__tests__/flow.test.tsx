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
const skip = enginePath ? false : '没有找到引擎，先运行 cargo build';

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const show = (label: string, frame: string | undefined) => {
	if (process.env['TIDYFS_SHOW_FRAMES']) console.log(`\n===== ${label} =====\n${frame ?? ''}`);
};

const KEY = {enter: '\r', escape: '\x1b', down: '\x1b[B', tab: '\t'};
/** 终端的“括号粘贴”：把文件夹拖进窗口或 Ctrl+V 时终端发来的就是这个。 */
const paste = (text: string) => `\x1b[200~${text}\x1b[201~`;

async function waitFor(read: () => string | undefined, text: string | RegExp, timeout = 10_000): Promise<string> {
	const started = Date.now();
	const matches = (frame: string) => (typeof text === 'string' ? frame.includes(text) : text.test(frame));
	while (Date.now() - started < timeout) {
		const frame = read() ?? '';
		if (matches(frame)) {
			// 画面出来后，新页面的按键监听在下一轮 effect 里才挂上。
			await sleep(100);
			return frame;
		}
		await sleep(30);
	}
	throw new Error(`等待“${text}”超时，最后一帧：\n${read()}`);
}

let base: string;
before(() => {
	base = fs.mkdtempSync(path.join(os.tmpdir(), 'tidyfs-ui-'));
	process.env['TIDYFS_JOURNAL_DIR'] = path.join(base, 'journals');
});
after(() => fs.rmSync(base, {recursive: true, force: true}));

/** 在临时目录里建一棵目录树：键是相对路径，值为字符串时写成文件。 */
function tree(name: string, entries: Record<string, string | null>): string {
	const root = path.join(base, name);
	for (const [relative, content] of Object.entries(entries)) {
		const target = path.join(root, relative);
		if (content === null) {
			fs.mkdirSync(target, {recursive: true});
		} else {
			fs.mkdirSync(path.dirname(target), {recursive: true});
			fs.writeFileSync(target, content);
		}
	}
	return root;
}

async function launch() {
	const engine = new EngineClient(enginePath!, pino({level: 'silent'}));
	const ui = render(<App engine={engine} log={pino({level: 'silent'})} />);
	await waitFor(ui.lastFrame, /v\d+\.\d+/);
	return {
		ui,
		frame: () => ui.lastFrame() ?? '',
		press: async (...keys: string[]) => {
			for (const key of keys) {
				ui.stdin.write(key);
				await sleep(60);
			}
		},
		close: () => {
			ui.unmount();
			engine.close();
		},
	};
}

test('清理空文件夹：粘贴路径 → 扫描 → 确认 → 执行', {skip}, async () => {
	const root = tree('empty-basic', {'a/b/c': null, 'keep/file.txt': 'x', 'only-noise/Thumbs.db': 'x'});
	const app = await launch();
	try {
		await app.press(paste(root));
		show('粘贴路径后', await waitFor(app.frame, '已添加并勾选'));

		await app.press(KEY.enter);
		const review = await waitFor(app.frame, '将删除');
		show('扫描结果', review);
		assert.match(review, /将删除 4 个空文件夹/);
		// 列表按路径排序，子文件夹紧跟在父文件夹后面。
		assert.ok(review.indexOf('a\\b\\c') > review.indexOf('a\\b'));

		await app.press(KEY.enter);
		show('确认', await waitFor(app.frame, '记进操作日志'));
		await app.press('y');

		const summary = await waitFor(app.frame, '全部完成');
		show('完成', summary);
		assert.match(summary, /操作日志/);
		assert.ok(!fs.existsSync(path.join(root, 'a')));
		assert.ok(!fs.existsSync(path.join(root, 'only-noise')));
		assert.ok(fs.existsSync(path.join(root, 'keep', 'file.txt')));
	} finally {
		app.close();
	}
});

test('选择位置：不存在的路径会提示，Esc 不会退出程序', {skip}, async () => {
	const app = await launch();
	try {
		await app.press(paste(path.join(base, 'does-not-exist')));
		show('粘贴不存在的路径', await waitFor(app.frame, '找不到'));

		await app.press('p', 'Z:\\nope-tidyfs', KEY.enter);
		const typed = await waitFor(app.frame, '找不到 Z:\\nope-tidyfs');
		assert.match(typed, /路径 Z:\\nope-tidyfs/, '路径不对时输入框保留，方便修改');

		await app.press(KEY.escape);
		await sleep(300);
		await app.press(KEY.escape);
		await sleep(300);
		assert.match(app.frame(), /在哪里/);
		assert.doesNotMatch(app.frame(), /路径 Z:/);
	} finally {
		app.close();
	}
});

test('确认页：跳过某个文件夹，它和它的上下级都原样保留', {skip}, async () => {
	const root = tree('empty-skip', {'a/b/c': null, other: null});
	const app = await launch();
	try {
		await app.press(paste(root));
		await waitFor(app.frame, '已添加并勾选');
		await app.press(KEY.enter);
		await waitFor(app.frame, '将删除 4 个空文件夹');

		// 排序后是 a、a\b、a\b\c、other；跳过 a\b。
		await app.press(KEY.down, ' ');
		const review = await waitFor(app.frame, '将删除 1 个空文件夹');
		show('跳过 a\\b 之后', review);
		assert.match(review, /保留·里面有跳过的/);

		await app.press(KEY.enter);
		const confirm = await waitFor(app.frame, '记进操作日志');
		assert.match(confirm, /跳过 3 项/);
		await app.press('y');
		await waitFor(app.frame, '全部完成');

		assert.ok(fs.existsSync(path.join(root, 'a', 'b', 'c')));
		assert.ok(!fs.existsSync(path.join(root, 'other')));
	} finally {
		app.close();
	}
});

test('确认页：结果超过预览上限时，跳过一项的数量按完整结果计算', {skip}, async () => {
	// 一个文件夹里有 2001 个空子文件夹：预览只带前 2000 项，父文件夹不在预览里。
	const entries: Record<string, null> = {};
	for (let i = 0; i <= 2000; i++) entries[`parent/c${String(i).padStart(4, '0')}`] = null;
	const root = tree('big', entries);
	const app = await launch();
	try {
		await app.press(paste(root));
		await waitFor(app.frame, '已添加并勾选');
		await app.press(KEY.enter);
		await waitFor(app.frame, '将删除 2,002 个空文件夹');

		// 跳过排在第一行的子文件夹，它的父文件夹（不在预览里）也删不掉了，所以是 2000 而不是 2001。
		await app.press(' ');
		show('跳过一项之后', await waitFor(app.frame, '将删除 2,000 个空文件夹'));

		await app.press(KEY.enter);
		assert.match(await waitFor(app.frame, '记进操作日志'), /跳过 2 项/);
		await app.press('y');
		await waitFor(app.frame, '全部完成', 30_000);

		assert.ok(fs.existsSync(path.join(root, 'parent', 'c0001')));
		assert.ok(!fs.existsSync(path.join(root, 'parent', 'c0000')));
		assert.ok(!fs.existsSync(path.join(root, 'parent', 'c2000')));
	} finally {
		app.close();
	}
});

test('确认页：跳过整个位置', {skip}, async () => {
	const first = tree('loc-1', {x: null});
	const second = tree('loc-2', {y: null});
	const app = await launch();
	try {
		await app.press(paste(`${first}\r\n${second}`));
		await waitFor(app.frame, '已添加并勾选 2 个文件夹');
		await app.press(KEY.enter);
		await waitFor(app.frame, '将删除 2 个空文件夹');

		await app.press('a');
		show('跳过第一个位置', await waitFor(app.frame, '将删除 1 个空文件夹'));
		await app.press(KEY.enter);
		await waitFor(app.frame, '记进操作日志');
		await app.press('y');
		await waitFor(app.frame, '全部完成');

		assert.ok(fs.existsSync(path.join(first, 'x')));
		assert.ok(!fs.existsSync(path.join(second, 'y')));
	} finally {
		app.close();
	}
});

test('拉平目录：切换功能和规则，预览写清楚从哪到哪', {skip}, async () => {
	const root = tree('20250101', {
		'album/backup/2025/photo.jpg': 'p',
		'project_alpha/archive_shell/video.mp4': 'v',
	});
	const app = await launch();
	try {
		await app.press(KEY.tab);
		const setup = await waitFor(app.frame, '保留第一层');
		show('拉平', setup);
		assert.match(setup, /整块磁盘不能直接拉平/);

		await app.press('m');
		await waitFor(app.frame, '只提一层');
		await app.press('m', 'm');
		await waitFor(app.frame, '保留第一层');

		await app.press(paste(root));
		await waitFor(app.frame, '已添加并勾选');
		await app.press(KEY.enter);
		const review = await waitFor(app.frame, '将拉平');
		show('拉平预览', review);
		assert.match(review, /将拉平 2 处，移动 2 个文件/);
		assert.match(review, /20250101\\album\\backup\\2025\s+→\s+20250101\\album\b/);

		await app.press(KEY.enter);
		await waitFor(app.frame, '记进操作日志');
		await app.press('y');
		const summary = await waitFor(app.frame, '全部完成');
		show('拉平完成', summary);

		assert.ok(fs.existsSync(path.join(root, 'album', 'photo.jpg')));
		assert.ok(fs.existsSync(path.join(root, 'project_alpha', 'video.mp4')));
		assert.ok(!fs.existsSync(path.join(root, 'album', 'backup')));
		assert.ok(!fs.existsSync(path.join(root, 'project_alpha', 'archive_shell')));
	} finally {
		app.close();
	}
});
