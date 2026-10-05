import {spawn} from 'node:child_process';
import stringWidth from 'string-width';

const numberFormat = new Intl.NumberFormat('zh-CN');

export const formatCount = (value: number): string => numberFormat.format(value);

export function formatBytes(bytes: number): string {
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit++;
	}
	return `${value.toFixed(value >= 100 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatDuration(ms: number): string {
	if (ms < 1000) return `${ms}ms`;
	const seconds = ms / 1000;
	if (seconds < 60) return `${seconds.toFixed(1)}s`;
	const minutes = Math.floor(seconds / 60);
	return `${minutes}m${Math.round(seconds % 60)}s`;
}

/** 按显示宽度截断（中文占两格），中间用 … 代替，保留路径首尾。尾部（文件名）分到更多宽度。 */
export function truncateMiddle(text: string, width: number): string {
	if (stringWidth(text) <= width) return text;
	if (width <= 1) return width === 1 ? '…' : '';
	const budget = width - 1;
	const tailBudget = Math.ceil(budget * 0.6);
	const headBudget = budget - tailBudget;
	const chars = [...text];

	let head = '';
	for (const char of chars) {
		if (stringWidth(head + char) > headBudget) break;
		head += char;
	}
	let tail = '';
	for (let i = chars.length - 1; i >= 0; i--) {
		if (stringWidth(chars[i]! + tail) > tailBudget) break;
		tail = chars[i]! + tail;
	}
	return `${head}…${tail}`;
}

export function padEnd(text: string, width: number): string {
	const visible = stringWidth(text);
	return visible >= width ? text : text + ' '.repeat(width - visible);
}

/** 把绝对路径显示成相对根目录的形式。 */
export function relativeTo(root: string, target: string): string {
	if (target.toLowerCase().startsWith(root.toLowerCase())) {
		const rest = target.slice(root.length).replace(/^[\\/]+/, '');
		return rest === '' ? '.' : rest;
	}
	return target;
}

/** 用系统默认程序打开文件或目录。 */
export function openExternal(target: string): void {
	const command = process.platform === 'win32' ? 'explorer.exe' : process.platform === 'darwin' ? 'open' : 'xdg-open';
	spawn(command, [target], {detached: true, stdio: 'ignore'}).unref();
}

export function isVolumeRoot(target: string): boolean {
	return /^[a-zA-Z]:[\\/]?$/.test(target) || target === '/';
}
