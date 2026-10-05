import {Box, Text, useInput, usePaste, useWindowSize} from 'ink';
import {useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {Hello} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Meter, Notice, PathText, Segmented, type Hint} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {formatBytes, isVolumeRoot, padEnd, truncateMiddle} from '../lib/format.js';
import {MODES, TASK_LABEL, addFolders, type Settings, type Target} from '../state.js';

interface Props {
	engine: EngineClient;
	hello: Hello | null;
	targets: Target[];
	setTargets: (update: (previous: Target[]) => Target[]) => void;
	settings: Settings;
	setSettings: (settings: Settings) => void;
	refreshDrives: () => void;
	onStart: (roots: string[]) => void;
	onQuit: () => void;
	error: string | null;
}

const DRIVE_KIND: Record<string, string> = {
	fixed: '本地磁盘',
	removable: '可移动磁盘',
	network: '网络位置',
	cdrom: '光驱',
	ramdisk: '内存盘',
	unknown: '磁盘',
};

const TASKS = (['empty', 'flatten'] as const).map(value => ({value, label: TASK_LABEL[value]}));

export function Setup({engine, hello, targets, setTargets, settings, setSettings, refreshDrives, onStart, onQuit, error}: Props) {
	const {columns} = useWindowSize();
	const [cursor, setCursor] = useState(0);
	const [input, setInput] = useState<string | null>(null);
	const [notice, setNotice] = useState<string | null>(null);
	const [picking, setPicking] = useState(false);

	const flatten = settings.task === 'flatten';
	const disabled = (target: Target) => flatten && isVolumeRoot(target.path);
	const runnable = targets.filter(target => target.selected && !disabled(target));
	const current = targets[Math.min(cursor, targets.length - 1)];
	const modeInfo = MODES.find(item => item.mode === settings.mode)!;

	const pick = () => {
		setPicking(true);
		setNotice('文件夹选择框已打开（支持多选）。没看到的话，看一下任务栏');
		engine
			.request({cmd: 'pick'}, 'picked')
			.then(({paths}) => {
				if (paths.length > 0) {
					setTargets(previous => addFolders(previous, paths));
					setNotice(`已添加 ${paths.length} 个文件夹`);
				} else {
					setNotice(null);
				}
			})
			.catch((caught: Error) => setNotice(caught.message))
			.finally(() => setPicking(false));
	};

	const toggle = (target: Target) => {
		if (disabled(target)) return setNotice('拉平需要具体的文件夹，不能直接作用于整块磁盘');
		setTargets(previous => previous.map(item => (item === target ? {...item, selected: !item.selected} : item)));
	};

	usePaste(text => setInput(value => (value ?? '') + text.replace(/[\r\n]+/g, '')), {isActive: input !== null});

	useInput(
		(char, key) => {
			if (key.escape) return setInput(null);
			if (key.return) {
				const value = (input ?? '').trim();
				setInput(null);
				if (value !== '') {
					setTargets(previous => addFolders(previous, [value]));
					setCursor(targets.length);
				}
				return;
			}
			if (key.backspace || key.delete) return setInput(value => (value ?? '').slice(0, -1));
			if (char && !key.ctrl && !key.meta) setInput(value => (value ?? '') + char);
		},
		{isActive: input !== null},
	);

	useInput(
		(char, key) => {
			setNotice(null);
			if (char === 'q' || key.escape) return onQuit();
			if (key.upArrow || char === 'k') return setCursor(value => Math.max(0, value - 1));
			if (key.downArrow || char === 'j') return setCursor(value => Math.min(targets.length - 1, value + 1));
			if (key.tab || key.leftArrow || key.rightArrow) return setSettings({...settings, task: flatten ? 'empty' : 'flatten'});
			if (char === 'm' && flatten) {
				const index = MODES.findIndex(item => item.mode === settings.mode);
				return setSettings({...settings, mode: MODES[(index + 1) % MODES.length]!.mode});
			}
			if (char === ' ' && current) return toggle(current);
			if (char === 'a') {
				const enable = targets.some(target => !target.selected && !disabled(target));
				return setTargets(previous => previous.map(target => ({...target, selected: enable && !disabled(target)})));
			}
			if (char === 'f' && !picking) return pick();
			if (char === 'p') return setInput('');
			if (char === 'r') {
				refreshDrives();
				return setNotice('磁盘列表已刷新');
			}
			if ((char === 'x' || key.delete) && current?.source === 'folder') {
				setTargets(previous => previous.filter(target => target !== current));
				return setCursor(value => Math.max(0, value - 1));
			}
			if (key.return) {
				if (runnable.length === 0) return setNotice('先用空格勾选至少一个位置');
				return onStart(runnable.map(target => target.path));
			}
		},
		{isActive: input === null},
	);

	const hints: Hint[] =
		input !== null
			? [
					['Enter', '添加'],
					['Esc', '取消'],
				]
			: [
					['↑↓', '移动'],
					['空格', '勾选'],
					['a', '全选'],
					['Tab', '切换功能'],
					...(flatten ? ([['m', '切换规则']] as Hint[]) : []),
					['f', '选择文件夹'],
					['p', '粘贴路径'],
					['x', '移除'],
					['r', '刷新'],
					['Enter', '开始扫描'],
					['q', '退出'],
				];

	// 宽窗口显示卷标、文件系统和容量条；窄窗口只保留路径和容量。
	const wide = columns >= 96;
	const pathWidth = wide ? Math.max(10, Math.min(36, columns - 72)) : Math.max(8, Math.min(24, columns - 30));
	const meterWidth = wide ? 14 : 6;

	return (
		<Frame step="选择位置" hints={hints} status={hello ? `v${hello.version}` : undefined}>
			<Box flexDirection="column" marginBottom={1}>
				<Segmented options={TASKS} value={settings.task} />
				<Box marginTop={1}>
					{flatten ? (
						<Text>
							<Text color={color.subtle}>规则 </Text>
							<Text color={color.violet}>{modeInfo.label}</Text>
							<Text color={color.subtle}>
								{'  '}
								{modeInfo.description}
							</Text>
						</Text>
					) : (
						<Text color={color.subtle}>删除没有任何内容的文件夹。隐藏文件和 desktop.ini、Thumbs.db 这类噪音文件不算内容，会一起清理。</Text>
					)}
				</Box>
			</Box>

			<Group label="位置" right={`已选 ${runnable.length} · 不同磁盘并行扫描`}>
				{targets.length === 0 ? <Text color={color.subtle}>正在读取磁盘…</Text> : null}
				{targets.map((target, index) => {
					const active = index === cursor;
					const off = disabled(target);
					const mark = off ? glyph.disabled : target.selected ? glyph.checked : glyph.unchecked;
					const markColor = off ? color.faint : target.selected ? color.accent : color.subtle;
					const drive = target.drive;
					const used = drive && drive.totalBytes > 0 ? 1 - drive.freeBytes / drive.totalBytes : 0;
					return (
						<Box key={target.path}>
							<Text color={color.accent}>{active ? `${glyph.pointer} ` : '  '}</Text>
							<Text color={markColor}>{mark} </Text>
							<Box width={pathWidth + 2}>
								{off ? (
									<Text color={color.faint}>{padEnd(truncateMiddle(target.path, pathWidth), pathWidth)}</Text>
								) : (
									<PathText path={target.path} width={pathWidth} strong={active} />
								)}
							</Box>
							{drive ? (
								<Text wrap="truncate">
									{wide ? (
										<Text color={off ? color.faint : color.subtle}>
											{padEnd(truncateMiddle(drive.label || DRIVE_KIND[drive.kind] || '磁盘', 14), 16)}
											{padEnd(drive.fileSystem, 7)}
										</Text>
									) : null}
									<Meter
										ratio={used}
										width={meterWidth}
										tint={off ? color.faint : used > 0.9 ? color.danger : used > 0.75 ? color.warn : color.accent}
									/>
									<Text color={off ? color.faint : color.subtle}>
										{'  '}
										{wide
											? padEnd(`${formatBytes(drive.freeBytes)} 可用`, 12) + formatBytes(drive.totalBytes)
											: `${formatBytes(drive.freeBytes)} 可用`}
									</Text>
								</Text>
							) : (
								<Text color={color.subtle}>文件夹</Text>
							)}
						</Box>
					);
				})}
			</Group>

			{input !== null ? (
				<Box borderStyle="round" borderColor={color.accent} paddingX={1}>
					<Text color={color.accent}>路径 </Text>
					<Text>{input}</Text>
					<Text backgroundColor={color.accent}> </Text>
				</Box>
			) : null}
			<Box flexDirection="column">
				{error ? <Notice tone="danger">{error}</Notice> : null}
				{notice ? <Notice tone="info">{notice}</Notice> : null}
				{flatten && targets.some(target => target.source === 'drive') ? (
					<Notice tone="info">拉平只作用于具体文件夹，整块磁盘已置灰。按 f 或 p 添加文件夹。</Notice>
				) : null}
			</Box>
		</Frame>
	);
}
