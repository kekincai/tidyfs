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

export const TASK_LABEL: Record<TaskKind, string> = {
	empty: '清理空文件夹',
	flatten: '拉平目录',
};

export const MODES: {mode: FlattenMode; label: string; description: string}[] = [
	{mode: 'keep-endpoints', label: '保留首尾', description: '按日期目录分组，保留日期内第一层，去掉更深的壳目录'},
	{mode: 'one-level', label: '提升一层', description: '只把最内层的文件提升一层'},
	{mode: 'collapse-chain', label: '压到起点', description: '单链目录一路压平到链的起点'},
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
