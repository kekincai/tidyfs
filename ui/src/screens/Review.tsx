import {Box, Text, useInput, useWindowSize} from 'ink';
import {useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {Item, RootScan} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Callout, Notice, PathText, type Hint} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {formatCount, formatDuration, openExternal, relativeTo, truncateMiddle} from '../lib/format.js';
import {TASK_LABEL, type Settings} from '../state.js';

interface Props {
	engine: EngineClient;
	settings: Settings;
	scanId: number;
	results: RootScan[];
	onApply: (roots: number[]) => void;
	onBack: () => void;
	onQuit: () => void;
}

const actionable = (result: RootScan) => !result.error && result.count > 0;

function ItemLine({root, item, width}: {root: string; item: Item; width: number}) {
	if ('path' in item) return <PathText path={relativeTo(root, item.path)} width={width} />;
	const half = Math.floor((width - 16) / 2);
	return (
		<Text>
			<PathText path={relativeTo(root, item.from)} width={half} />
			<Text color={color.accent}> {glyph.arrow} </Text>
			<PathText path={relativeTo(root, item.to)} width={half} strong />
			<Text color={color.subtle}> {String(item.files).padStart(5)} 个文件</Text>
		</Text>
	);
}

export function Review({engine, settings, scanId, results, onApply, onBack, onQuit}: Props) {
	const {columns, rows} = useWindowSize();
	const [active, setActive] = useState(() => Math.max(0, results.findIndex(actionable)));
	const [included, setIncluded] = useState(() => new Set(results.filter(actionable).map(result => result.root)));
	const [offset, setOffset] = useState(0);
	const [confirming, setConfirming] = useState(false);
	const [notice, setNotice] = useState<string | null>(null);

	const current = results[active];
	const items = current?.items ?? [];
	const listHeight = Math.max(3, rows - 17 - (confirming ? 4 : 0));
	const selected = results.filter(result => included.has(result.root) && actionable(result));
	const selectedCount = selected.reduce((sum, result) => sum + result.count, 0);
	const unit = settings.task === 'empty' ? '个空文件夹' : '组目录';

	const moveActive = (delta: number) => {
		setActive(value => (value + delta + results.length) % results.length);
		setOffset(0);
	};
	const scroll = (delta: number) => setOffset(value => Math.max(0, Math.min(Math.max(0, items.length - listHeight), value + delta)));

	useInput((char, key) => {
		if (confirming) {
			if (char === 'y' || char === 'Y') return onApply(selected.map(result => result.root));
			if (char === 'n' || char === 'N' || key.escape) return setConfirming(false);
			return;
		}
		setNotice(null);
		if (char === 'q') return onQuit();
		if (key.escape) return onBack();
		if (key.leftArrow || (key.tab && key.shift)) return moveActive(-1);
		if (key.rightArrow || key.tab) return moveActive(1);
		if (key.upArrow || char === 'k') return scroll(-1);
		if (key.downArrow || char === 'j') return scroll(1);
		if (key.pageUp) return scroll(-listHeight);
		if (key.pageDown) return scroll(listHeight);
		if (key.home) return setOffset(0);
		if (key.end) return scroll(items.length);
		if (char === ' ' && current) {
			if (!actionable(current)) return setNotice('这个位置没有可执行的内容');
			return setIncluded(previous => {
				const next = new Set(previous);
				if (next.has(current.root)) next.delete(current.root);
				else next.add(current.root);
				return next;
			});
		}
		if (char === 'o' && current && actionable(current)) {
			engine
				.request({cmd: 'export', scan: scanId, root: current.root}, 'exported')
				.then(({path}) => {
					openExternal(path);
					setNotice(`完整清单已导出到 ${path}`);
				})
				.catch((caught: Error) => setNotice(caught.message));
			return;
		}
		if (key.return) {
			if (selected.length === 0) return setNotice('没有勾选任何要执行的位置');
			return setConfirming(true);
		}
	});

	const hints: Hint[] = confirming
		? [
				['y', '确认执行'],
				['n', '再想想'],
			]
		: [
				['←→', '切换位置'],
				['↑↓', '滚动'],
				['PgUp PgDn', '翻页'],
				['空格', '包含 / 排除'],
				['o', '导出完整清单'],
				['Enter', '执行'],
				['Esc', '返回'],
			];

	const listWidth = Math.max(20, columns - 14);
	const visible = items.slice(offset, offset + listHeight);
	const hidden = current ? current.count - items.length : 0;
	const meta = current?.stats
		? [
				`${formatCount(current.stats.dirs)} 个目录`,
				formatDuration(current.elapsedMs),
				current.stats.skipped ? `跳过 ${formatCount(current.stats.skipped)} 个系统目录` : '',
				current.stats.errors ? `${formatCount(current.stats.errors)} 处无法读取` : '',
			]
				.filter(Boolean)
				.join(` ${glyph.dot} `)
		: '';

	return (
		<Frame
			step="确认"
			hints={hints}
			status={
				<Text>
					{TASK_LABEL[settings.task]} {glyph.dot} 将处理{' '}
					<Text color={color.accent} bold>
						{formatCount(selectedCount)}
					</Text>{' '}
					{unit}
				</Text>
			}
		>
			<Box flexWrap="wrap" columnGap={1} marginBottom={1}>
				{results.map((result, index) => {
					const on = included.has(result.root) && actionable(result);
					const mark = result.error ? glyph.fail : on ? glyph.checked : glyph.unchecked;
					const label = ` ${mark} ${truncateMiddle(result.path, 22)}  ${result.error ? '失败' : formatCount(result.count)} `;
					if (index === active) {
						return (
							<Text key={result.root} backgroundColor={color.accent} color="#1A1B26" bold>
								{label}
							</Text>
						);
					}
					return (
						<Text key={result.root} color={result.error ? color.danger : on ? undefined : color.subtle}>
							{label}
						</Text>
					);
				})}
			</Box>

			{current ? (
				<Group label={truncateMiddle(current.path, 40)} right={meta}>
					{current.error ? (
						<Notice tone="danger">{current.error}</Notice>
					) : current.count === 0 ? (
						<Box paddingY={1}>
							<Notice tone="ok">这里很干净，没有需要处理的内容</Notice>
						</Box>
					) : (
						<Box flexDirection="column" height={listHeight}>
							{visible.map((item, index) => (
								<Box key={offset + index}>
									<Text color={color.faint}>{String(offset + index + 1).padStart(6)} </Text>
									<ItemLine root={current.path} item={item} width={listWidth} />
								</Box>
							))}
						</Box>
					)}
					{current.count > 0 ? (
						<Text color={color.subtle}>
							{'        '}
							{formatCount(offset + 1)}–{formatCount(Math.min(offset + listHeight, items.length))} / {formatCount(current.count)}
							{hidden > 0 ? `  ${glyph.dot}  只预览前 ${formatCount(items.length)} 项，按 o 查看全部` : ''}
							{current.ignoredFiles > 0 ? `  ${glyph.dot}  连带清理 ${formatCount(current.ignoredFiles)} 个隐藏 / 噪音文件` : ''}
							{settings.task === 'flatten' ? `  ${glyph.dot}  移动 ${formatCount(current.fileCount)} 个文件` : ''}
						</Text>
					) : null}
				</Group>
			) : null}

			{confirming ? (
				<Callout tone="warn" title={`在 ${selected.length} 个位置${TASK_LABEL[settings.task]}，共 ${formatCount(selectedCount)} ${unit}`}>
					<Text color={color.subtle}>每一步都会写进操作日志，方便事后核对。按 y 开始，n 返回。</Text>
				</Callout>
			) : null}
			{notice ? <Notice tone="info">{notice}</Notice> : null}
		</Frame>
	);
}
