import type {Drive, FlattenMode, TaskKind} from './engine/protocol.js';

export interface Target {
	path: string;
	source: 'drive' | 'folder';
	drive?: Drive;
	selected: boolean;
}

export interface Settings {
	task: TaskKind;
	mode: FlattenMode;
}

/** 预览里被排除的项：序号对应扫描结果里的 `items`。 */
export interface Exclusion {
	root: number;
	items: number[];
}

export const TASK_LABEL: Record<TaskKind, string> = {
	empty: '清理空文件夹',
	flatten: '拉平目录',
};

export const TASK_INTRO: Record<TaskKind, string[]> = {
	empty: ['删除里面什么都没有的文件夹。', '只有隐藏文件、desktop.ini、Thumbs.db 的也算空，会一起删掉。'],
	flatten: ['把套了很多层的文件夹拉平：深处的文件往上移，移空的中间文件夹删掉。'],
};

export const MODES: {mode: FlattenMode; label: string; description: string; example: [before: string, after: string]}[] = [
	{
		mode: 'keep-endpoints',
		label: '保留第一层',
		description: '日期文件夹（如 20250101）里只留第一层文件夹，更深处的文件都提到这一层',
		example: ['20250101\\相册\\备份\\2025\\a.jpg', '20250101\\相册\\a.jpg'],
	},
	{
		mode: 'one-level',
		label: '只提一层',
		description: '只有一个子文件夹的“套娃”目录，把最里面的文件往上提一层',
		example: ['相册\\备份\\2025\\a.jpg', '相册\\备份\\a.jpg'],
	},
	{
		mode: 'collapse-chain',
		label: '压到最外层',
		description: '只有一个子文件夹的“套娃”目录，一路压到最外层',
		example: ['相册\\备份\\2025\\a.jpg', '相册\\a.jpg'],
	},
];

/** 刷新磁盘列表时保留用户添加的文件夹和已有的勾选状态。 */
export function mergeDrives(previous: Target[], drives: Drive[]): Target[] {
	const selected = new Set(previous.filter(target => target.selected).map(target => target.path.toLowerCase()));
	const driveTargets = drives.map<Target>(drive => ({
		path: drive.path,
		source: 'drive',
		drive,
		selected: selected.has(drive.path.toLowerCase()),
	}));
	return [...driveTargets, ...previous.filter(target => target.source === 'folder')];
}

export function addFolders(previous: Target[], paths: string[]): Target[] {
	let next = [...previous];
	for (const raw of paths) {
		const path = raw.trim().replace(/^"(.*)"$/, '$1');
		if (path === '') continue;
		const key = path.toLowerCase();
		if (next.some(target => target.path.toLowerCase() === key)) {
			next = next.map(target => (target.path.toLowerCase() === key ? {...target, selected: true} : target));
		} else {
			next.push({path, source: 'folder', selected: true});
		}
	}
	return next;
}

/**
 * 把粘贴或拖进窗口的文字拆成路径。每行一个；拖入多个文件夹时终端会写成
 * `"C:\有 空格" D:\无空格` 这样一行，有引号时按引号和空格拆开。
 */
export function parsePaths(text: string): string[] {
	return text
		.split(/[\r\n]+/)
		.flatMap(line => (line.includes('"') ? [...line.matchAll(/"([^"]+)"|(\S+)/g)].map(match => match[1] ?? match[2] ?? '') : [line]))
		.map(path => path.trim())
		.filter(path => path !== '');
}
