import {Box, Text, useInput, usePaste, useWindowSize} from 'ink';
import {useRef, useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {Hello} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Meter, Notice, PathText, Segmented, type Hint} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {checkFolder, formatBytes, isVolumeRoot, padEnd, truncateMiddle} from '../lib/format.js';
import {MODES, TASK_INTRO, TASK_LABEL, addFolders, parsePaths, type Settings, type Target} from '../state.js';

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

type Tone = 'info' | 'ok' | 'warn';

export function Setup({engine, hello, targets, setTargets, settings, setSettings, refreshDrives, onStart, onQuit, error}: Props) {
	const {columns} = useWindowSize();
	const [cursor, setCursor] = useState(0);
	const [input, setInput] = useState<string | null>(null);
	const [notice, setNotice] = useState<{tone: Tone; text: string} | null>(null);
	const [picking, setPicking] = useState(false);

	const flatten = settings.task === 'flatten';
	// 拉平不能用在整块磁盘上：拉平时磁盘不出现在列表里（勾选状态保留，切回清理时还在）。
	const usable = (target: Target) => !(flatten && isVolumeRoot(target.path));
	const shown = targets.filter(usable);
	const runnable = shown.filter(target => target.selected);
	const current = shown[Math.min(cursor, shown.length - 1)];
	// 文件夹选择框是异步返回的，加入时要基于最新的列表。
	const latest = useRef(targets);
	latest.current = targets;
	const modeInfo = MODES.find(item => item.mode === settings.mode)!;
	const say = (text: string, tone: Tone = 'info') => setNotice({tone, text});

	/** 校验并加入文件夹，光标移到第一个加入的位置。返回没加进去的原因。 */
	const add = (raw: string[]): string[] => {
		const problems: string[] = [];
		const paths: string[] = [];
		for (const item of raw) {
			const checked = checkFolder(item);
			if ('error' in checked) problems.push(checked.error);
			else if (flatten && isVolumeRoot(checked.path)) problems.push(`拉平不能直接用在整块磁盘 ${checked.path} 上，请选里面的文件夹`);
			else paths.push(checked.path);
		}
		if (paths.length > 0) {
			const next = addFolders(latest.current, paths);
			latest.current = next;
			setTargets(() => next);
			const first = next.filter(usable).findIndex(target => target.path.toLowerCase() === paths[0]!.toLowerCase());
			setCursor(Math.max(0, first));
		}
		if (problems.length > 0) say(problems.join('；'), 'warn');
		else if (paths.length > 0) say(paths.length === 1 ? `已添加并勾选 ${paths[0]}` : `已添加并勾选 ${paths.length} 个文件夹`, 'ok');
		return problems;
	};

	const pick = () => {
		setPicking(true);
		say('文件夹选择框已打开（可以多选）。没看到的话，看一下任务栏');
		engine
			.request({cmd: 'pick'}, 'picked')
			.then(({paths}) => (paths.length > 0 ? void add(paths) : setNotice(null)))
			.catch((caught: Error) => say(caught.message, 'warn'))
			.finally(() => setPicking(false));
	};

	const toggle = (target: Target) => {
		setTargets(previous => previous.map(item => (item === target ? {...item, selected: !item.selected} : item)));
	};

	const start = () => {
		if (runnable.length > 0) return onStart(runnable.map(target => target.path));
		// 什么都没勾选时，直接扫描光标所在的位置。扫描只是预览，不会改动任何东西。
		if (current) return onStart([current.path]);
		say(flatten ? '先按 f 选择要拉平的文件夹' : '先用空格勾选要扫描的位置', 'warn');
	};

	// 粘贴或把文件夹拖进窗口：输入框打开时填进输入框，否则直接加入列表。
	usePaste(text => {
		if (input !== null) return setInput(value => (value ?? '') + text.replace(/[\r\n]+/g, ''));
		add(parsePaths(text));
	});

	useInput(
		(char, key) => {
			if (key.escape) return setInput(null);
			if (key.return) {
				const value = (input ?? '').trim();
				if (value === '') return setInput(null);
				// 路径不对时保留输入框，方便直接改。
				if (add(parsePaths(value)).length === 0) setInput(null);
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
			if (char === 'q') return onQuit();
			if (key.upArrow || char === 'k') return setCursor(value => Math.max(0, value - 1));
			if (key.downArrow || char === 'j') return setCursor(value => Math.min(shown.length - 1, value + 1));
			if (key.tab || key.leftArrow || key.rightArrow) {
				setCursor(0);
				return setSettings({...settings, task: flatten ? 'empty' : 'flatten'});
			}
			if (char === 'm' && flatten) {
				const index = MODES.findIndex(item => item.mode === settings.mode);
				return setSettings({...settings, mode: MODES[(index + 1) % MODES.length]!.mode});
			}
			if (char === ' ' && current) return toggle(current);
			if (char === 'a') {
				const enable = shown.some(target => !target.selected);
				return setTargets(previous => previous.map(target => (usable(target) ? {...target, selected: enable} : target)));
			}
			if (char === 'f') return picking ? say('文件夹选择框已经打开了，看一下任务栏') : pick();
			if (char === 'p') return setInput('');
			if (char === 'r') {
				refreshDrives();
				return say('磁盘列表已刷新');
			}
			if ((char === 'x' || key.delete) && current?.source === 'folder') {
				setTargets(previous => previous.filter(target => target !== current));
				return setCursor(value => Math.max(0, value - 1));
			}
			if (key.return) return start();
			// 不支持“括号粘贴”的终端会把粘贴内容当成一串字符送进来。
			if (char.length > 1 && /[\\/]/.test(char)) add(parsePaths(char));
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
					['Tab', '换功能'],
					...(flatten ? ([['m', '换规则']] as Hint[]) : []),
					['f', '添加文件夹'],
					...(current?.source === 'folder' ? ([['x', '移出列表']] as Hint[]) : []),
					['Enter', '开始扫描'],
					['q', '退出'],
				];

	// 宽窗口显示卷标、文件系统和容量条；窄窗口只保留路径和容量。
	const wide = columns >= 96;
	const pathWidth = wide ? Math.max(10, Math.min(36, columns - 72)) : Math.max(8, Math.min(24, columns - 30));
	const meterWidth = wide ? 14 : 6;
	const hiddenDrives = targets.length - shown.length;

	return (
		<Frame
			step="选择位置"
			hints={hints}
			status={hello ? `v${hello.version}` : undefined}
			footer={
				<>
					{input !== null ? (
						<Box borderStyle="round" borderColor={color.accent} paddingX={1}>
							<Text color={color.accent}>路径 </Text>
							<Text>{input}</Text>
							<Text backgroundColor={color.accent}> </Text>
						</Box>
					) : null}
					<Box flexDirection="column">
						{error ? <Notice tone="danger">{error}</Notice> : null}
						{notice ? <Notice tone={notice.tone}>{notice.text}</Notice> : null}
					</Box>
				</>
			}
		>
			<Box flexDirection="column" marginBottom={1}>
				<Box columnGap={2}>
					<Text color={color.subtle}>要做什么</Text>
					<Segmented options={TASKS} value={settings.task} />
					<Text color={color.faint}>Tab 切换</Text>
				</Box>
				<Box marginTop={1} flexDirection="column">
					{TASK_INTRO[settings.task].map(line => (
						<Text key={line} wrap="truncate-end">
							{line}
						</Text>
					))}
					{flatten ? (
						<Box flexDirection="column">
							<Text wrap="truncate-end">
								<Text color={color.subtle}>规则 </Text>
								<Text color={color.violet} bold>
									{modeInfo.label}
								</Text>
								<Text color={color.faint}>
									{'  '}m 切换（共 {MODES.length} 种）
								</Text>
							</Text>
							<Text color={color.subtle} wrap="truncate-end">
								{'     '}
								{modeInfo.description}
							</Text>
							<Text wrap="truncate-end">
								<Text color={color.subtle}>例如 </Text>
								<Text color={color.subtle}>{modeInfo.example[0]}</Text>
								<Text color={color.accent}> {glyph.arrow} </Text>
								<Text>{modeInfo.example[1]}</Text>
							</Text>
						</Box>
					) : (
						<Text color={color.subtle} wrap="truncate-end">
							扫描后会先列出清单给你确认，确认之前不会改动任何文件。
						</Text>
					)}
				</Box>
			</Box>

			<Group label="在哪里" right={runnable.length > 0 ? `已勾选 ${runnable.length} 个 · 不同磁盘同时扫描` : '空格勾选，可以多选'}>
				{targets.length === 0 ? <Text color={color.subtle}>正在读取磁盘…</Text> : null}
				{shown.map((target, index) => {
					const active = index === cursor || (index === shown.length - 1 && cursor >= shown.length);
					const mark = target.selected ? glyph.checked : glyph.unchecked;
					const markColor = target.selected ? color.accent : color.subtle;
					const drive = target.drive;
					const used = drive && drive.totalBytes > 0 ? 1 - drive.freeBytes / drive.totalBytes : 0;
					return (
						<Box key={target.path}>
							<Text color={color.accent}>{active ? `${glyph.pointer} ` : '  '}</Text>
							<Text color={markColor}>{mark} </Text>
							<Box width={pathWidth + 2}>
								<PathText path={target.path} width={pathWidth} strong={active} />
							</Box>
							{drive ? (
								<Text wrap="truncate">
									{wide ? (
										<Text color={color.subtle}>
											{padEnd(truncateMiddle(drive.label || DRIVE_KIND[drive.kind] || '磁盘', 14), 16)}
											{padEnd(drive.fileSystem, 7)}
										</Text>
									) : null}
									<Meter ratio={used} width={meterWidth} tint={used > 0.9 ? color.danger : used > 0.75 ? color.warn : color.accent} />
									<Text color={color.subtle}>
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
				{flatten && hiddenDrives > 0 ? (
					<Text color={color.faint}>
						{shown.length === 0 ? '还没有文件夹。' : ''}整块磁盘不能直接拉平，{hiddenDrives} 个磁盘已从列表隐藏
					</Text>
				) : null}
				<Box marginTop={shown.length > 0 ? 1 : 0}>
					<Text color={color.subtle} wrap="truncate-end">
						<Text color={color.accent}>＋</Text> 添加文件夹：按 <Text color={color.accent}>f</Text> 选择，或者把文件夹
						<Text bold>拖进这个窗口</Text>、粘贴路径（<Text color={color.accent}>p</Text> 手动输入）
					</Text>
				</Box>
			</Group>
		</Frame>
	);
}
