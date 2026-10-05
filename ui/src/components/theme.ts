/**
 * 一套克制的配色：正文用终端默认前景色（深浅背景都清晰），
 * 只有一个主色（蓝）负责“可操作 / 当前”，其余颜色只表达状态。
 */
export const color = {
	accent: '#7AA2F7',
	accentSoft: '#3D59A1',
	violet: '#BB9AF7',
	ok: '#9ECE6A',
	warn: '#E0AF68',
	danger: '#F7768E',
	subtle: '#7C8599',
	faint: '#4A5165',
} as const;

export const glyph = {
	brand: '◆',
	pointer: '›',
	checked: '●',
	unchecked: '○',
	disabled: '⊘',
	ok: '✓',
	fail: '✕',
	warn: '!',
	stopped: '■',
	arrow: '→',
	dot: '·',
	rule: '─',
	bar: '━',
} as const;

export const STEPS = ['选择位置', '扫描', '确认', '执行', '完成'] as const;
export type Step = (typeof STEPS)[number];
