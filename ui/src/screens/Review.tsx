import path from 'node:path';
import {Box, Text, useInput, useWindowSize} from 'ink';
import {useEffect, useMemo, useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {Item, RootScan} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Callout, Notice, PathText, type Hint} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {baseName, formatCount, formatDuration, isInside, isVolumeRoot, openExternal, relativeTo, truncateMiddle} from '../lib/format.js';
import {TASK_LABEL, type Exclusion, type Settings} from '../state.js';

interface Props {
	engine: EngineClient;
	settings: Settings;
	scanId: number;
	results: RootScan[];
	onApply: (roots: number[], exclude: Exclusion[]) => void;
	onBack: () => void;
	onQuit: () => void;
}

/** on：会处理；skipped：用户跳过（或在跳过的文件夹里面）；blocked：里面有要保留的文件夹，所以删不掉。 */
type RowState = 'on' | 'skipped' | 'blocked';

interface Row {
	index: number;
	item: Item;
	state: RowState;
}

const actionable = (result: RootScan) => !result.error && result.count > 0;
const itemPath = (item: Item) => ('path' in item ? item.path : item.from);
const samePath = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
const collator = new Intl.Collator('zh-CN', {numeric: true, sensitivity: 'base'});

/** 按路径逐段排序，子文件夹紧跟在父文件夹后面。 */
function comparePaths(a: string, b: string): number {
	const left = a.split(/[\\/]/);
	const right = b.split(/[\\/]/);
	for (let i = 0; i < Math.min(left.length, right.length); i++) {
		const order = collator.compare(left[i]!, right[i]!);
		if (order !== 0) return order;
	}
	return left.length - right.length;
}

function buildRows(result: RootScan, excluded: ReadonlySet<number>): Row[] {
	const rows = result.items.map((item, index) => ({index, item}));
	rows.sort((a, b) => comparePaths(itemPath(a.item), itemPath(b.item)));
	const kept = [...excluded].map(index => result.items[index]).filter((item): item is Item => item !== undefined);
	return rows.map(row => {
		if (!('path' in row.item)) return {...row, state: excluded.has(row.index) ? 'skipped' : 'on'};
		const target = row.item.path;
		// 和引擎一致：跳过一个空文件夹 = 它连同里面的子文件夹原样保留；它的上级文件夹也就不空了。
		if (kept.some(item => samePath(itemPath(item), target) || isInside(itemPath(item), target))) return {...row, state: 'skipped'};
		if (kept.some(item => isInside(target, itemPath(item)))) return {...row, state: 'blocked'};
		return {...row, state: 'on'};
	});
}

function ItemLine({root, row, width}: {root: string; row: Row; width: number}) {
	const muted = row.state !== 'on';
	const item = row.item;
	if ('path' in item) return <PathText path={relativeTo(root, item.path)} width={width} muted={muted} />;
	// 拉平的路径从所选文件夹的名字开始写，目标就是所选文件夹本身时也看得懂。
	const base = isVolumeRoot(root) ? root : path.dirname(root);
	// 箭头 3 列 + 文件数“ 12345 个文件” 13 列。
	const half = Math.max(8, Math.floor((width - 16) / 2));
	return (
		<Text>
			<PathText path={relativeTo(base, item.from)} width={half} muted={muted} />
			<Text color={muted ? color.faint : color.accent}> {glyph.arrow} </Text>
			<PathText path={relativeTo(base, item.to)} width={half} strong={!muted} muted={muted} />
			<Text color={color.subtle}> {String(item.files).padStart(5)} 个文件</Text>
		</Text>
	);
}

export function Review({engine, settings, scanId, results, onApply, onBack, onQuit}: Props) {
	const {columns, rows: windowRows} = useWindowSize();
	const [active, setActive] = useState(() => Math.max(0, results.findIndex(actionable)));
	const [excluded, setExcluded] = useState<Record<number, ReadonlySet<number>>>({});
	const [offRoots, setOffRoots] = useState<ReadonlySet<number>>(() => new Set());
	const [cursor, setCursor] = useState(0);
	const [offset, setOffset] = useState(0);
	const [confirming, setConfirming] = useState(false);
	const [notice, setNotice] = useState<string | null>(null);
	// 引擎按完整结果算出的数量，按“当时的排除项”记下，排除项一变旧数就不再用。
	const [exact, setExact] = useState<Record<number, {key: string; count: number; files: number}>>({});

	const flatten = settings.task === 'flatten';
	const rowsByRoot = useMemo(
		() => new Map(results.map(result => [result.root, buildRows(result, excluded[result.root] ?? new Set())])),
		[results, excluded],
	);
	const exclusionKey = (root: number) => [...(excluded[root] ?? [])].sort((a, b) => a - b).join(',');

	// 只列出了前几千项时，跳过一项可能连带保留列表之外的上级文件夹，只有引擎能算准。
	useEffect(() => {
		const exclude = results
			.filter(result => result.count > result.items.length && (excluded[result.root]?.size ?? 0) > 0)
			.map(result => ({root: result.root, items: [...excluded[result.root]!]}));
		if (exclude.length === 0) return;
		const keys = new Map(exclude.map(item => [item.root, exclusionKey(item.root)]));
		engine
			.request({cmd: 'tally', scan: scanId, exclude}, 'tally')
			.then(({roots}) =>
				setExact(previous => ({
					...previous,
					...Object.fromEntries(roots.map(item => [item.root, {key: keys.get(item.root) ?? '', count: item.count, files: item.fileCount}])),
				})),
			)
			// 算不出来时退回按列表估算。
			.catch(() => {});
	}, [excluded]);

	const tally = (result: RootScan) => {
		if (!actionable(result) || offRoots.has(result.root)) return {count: 0, files: 0, skipped: actionable(result) ? result.count : 0};
		const known = exact[result.root];
		if (known && known.key === exclusionKey(result.root))
			return {count: known.count, files: known.files, skipped: result.count - known.count};
		const skipped = (rowsByRoot.get(result.root) ?? []).filter(row => row.state !== 'on');
		const skippedFiles = skipped.reduce((sum, row) => sum + ('files' in row.item ? row.item.files : 0), 0);
		return {count: result.count - skipped.length, files: result.fileCount - skippedFiles, skipped: skipped.length};
	};
	const tallies = results.map(tally);
	const total = tallies.reduce((sum, item) => sum + item.count, 0);
	const totalFiles = tallies.reduce((sum, item) => sum + item.files, 0);
	const totalSkipped = tallies.reduce((sum, item) => sum + item.skipped, 0);
	const locations = results.filter((_, index) => tallies[index]!.count > 0);

	const current = results[active];
	const currentOff = current ? offRoots.has(current.root) : false;
	const rows = current ? (rowsByRoot.get(current.root) ?? []) : [];
	// 列表以外占用的行：框架 7（含两行按键提示）、分组标题、说明、页码、分组间距、提示各 1，
	// 多个位置时的标签行 2，确认框 6。确认时列表可以缩到 1 行，保证确认框看得见。
	const chrome = 12 + (results.length > 1 ? 2 : 0) + (confirming ? 6 : 0);
	const listHeight = Math.max(confirming ? 1 : 3, windowRows - chrome);

	const moveActive = (delta: number) => {
		setActive(value => (value + delta + results.length) % results.length);
		setCursor(0);
		setOffset(0);
	};
	const moveCursor = (delta: number) => {
		const next = Math.max(0, Math.min(rows.length - 1, cursor + delta));
		setCursor(next);
		setOffset(value => (next < value ? next : next >= value + listHeight ? next - listHeight + 1 : value));
	};

	const toggleRow = (row: Row) => {
		if (!current) return;
		const root = current.root;
		const set = new Set(excluded[root] ?? []);
		if (row.state === 'on') {
			set.add(row.index);
		} else if ('path' in row.item) {
			// 恢复一项：同一条路径上（上级、下级）的跳过都要取消，否则它还是删不掉。
			const target = row.item.path;
			for (const index of [...set]) {
				const other = itemPath(current.items[index]!);
				if (samePath(other, target) || isInside(other, target) || isInside(target, other)) set.delete(index);
			}
		} else {
			set.delete(row.index);
		}
		setExcluded(previous => ({...previous, [root]: set}));
	};

	const apply = () => {
		const roots = results.filter((_, index) => tallies[index]!.count > 0).map(result => result.root);
		const exclude = roots
			.map(root => ({root, items: [...(excluded[root] ?? [])].sort((a, b) => a - b)}))
			.filter(item => item.items.length > 0);
		onApply(roots, exclude);
	};

	useInput((char, key) => {
		if (confirming) {
			if (char === 'y' || char === 'Y') return apply();
			if (char === 'n' || char === 'N' || key.escape) return setConfirming(false);
			return;
		}
		setNotice(null);
		if (char === 'q') return onQuit();
		if (key.escape) return onBack();
		if (key.leftArrow || (key.tab && key.shift)) return moveActive(-1);
		if (key.rightArrow || key.tab) return moveActive(1);
		if (key.upArrow || char === 'k') return moveCursor(-1);
		if (key.downArrow || char === 'j') return moveCursor(1);
		if (key.pageUp) return moveCursor(-listHeight);
		if (key.pageDown) return moveCursor(listHeight);
		if (key.home) return moveCursor(-rows.length);
		if (key.end) return moveCursor(rows.length);
		if (!current) return;
		if (char === ' ') {
			if (!actionable(current)) return setNotice('这个位置没有要处理的内容');
			if (currentOff) return setNotice('整个位置已经跳过了，按 a 恢复');
			const row = rows[cursor];
			return row ? toggleRow(row) : undefined;
		}
		if (char === 'a') {
			if (!actionable(current)) return setNotice('这个位置没有要处理的内容');
			return setOffRoots(previous => {
				const next = new Set(previous);
				if (next.has(current.root)) next.delete(current.root);
				else next.add(current.root);
				return next;
			});
		}
		if (char === 'o') {
			if (!actionable(current)) return setNotice('这个位置没有要处理的内容');
			engine
				.request({cmd: 'export', scan: scanId, root: current.root}, 'exported')
				.then(({path: file}) => {
					openExternal(file);
					setNotice(`完整清单已导出并打开：${file}`);
				})
				.catch((caught: Error) => setNotice(caught.message));
			return;
		}
		if (key.return) {
			if (total === 0) return setNotice(totalSkipped > 0 ? '全部都跳过了，没有要执行的内容' : '没有需要处理的内容，按 Esc 返回');
			return setConfirming(true);
		}
	});

	const hints: Hint[] = confirming
		? [
				['y', '确定执行'],
				['n', '再看看'],
			]
		: [
				['↑↓', '选择'],
				['空格', '跳过 / 恢复这一项'],
				...(results.length > 1 ? ([['←→', '切换位置']] as Hint[]) : []),
				['a', '跳过 / 恢复整个位置'],
				['o', '导出完整清单'],
				['Enter', '执行…'],
				['Esc', '返回'],
			];

	// 每行：指针 2 + 勾选 2 + 路径 + 右侧说明（“  跳过”，或空文件夹的“  保留·里面有跳过的”）。
	const suffixWidth = rows.some(row => row.state === 'blocked') ? 20 : 6;
	const listWidth = Math.max(20, columns - 4 - 4 - suffixWidth);
	const visible = rows.slice(offset, offset + listHeight);
	const hidden = current ? current.count - current.items.length : 0;
	const meta = current?.stats
		? [
				`扫描了 ${formatCount(current.stats.dirs)} 个文件夹`,
				formatDuration(current.elapsedMs),
				current.stats.skipped ? `跳过 ${formatCount(current.stats.skipped)} 个系统 / 程序文件夹` : '',
				current.stats.errors ? `${formatCount(current.stats.errors)} 处没有权限读取` : '',
			]
				.filter(Boolean)
				.join(` ${glyph.dot} `)
		: '';
	const plan = flatten
		? `将拉平 ${formatCount(total)} 处，移动 ${formatCount(totalFiles)} 个文件`
		: `将删除 ${formatCount(total)} 个空文件夹`;

	return (
		<Frame
			step="确认"
			hints={hints}
			footer={
				<>
					{confirming ? (
						<Callout
							tone="warn"
							title={
								flatten
									? `确定拉平 ${formatCount(total)} 处、移动 ${formatCount(totalFiles)} 个文件吗？`
									: `确定删除 ${formatCount(total)} 个空文件夹吗？`
							}
						>
							<Text color={color.subtle} wrap="truncate-end">
								{`涉及 ${locations.length} 个位置${totalSkipped > 0 ? `，跳过 ${formatCount(totalSkipped)} 项` : ''}。${flatten ? '同名文件会自动改名，不会覆盖。' : '空文件夹直接删除，不进回收站。'}`}
							</Text>
							<Text wrap="truncate-end">
								<Text color={color.subtle}>每一步都会记进操作日志。</Text>按 <Text color={color.accent}>y</Text> 开始，
								<Text color={color.accent}>n</Text> 返回。
							</Text>
						</Callout>
					) : null}
					{notice ? <Notice tone="info">{notice}</Notice> : null}
				</>
			}
			status={
				<Text>
					{TASK_LABEL[settings.task]} {glyph.dot}{' '}
					<Text color={total > 0 ? color.accent : color.subtle} bold>
						{plan}
					</Text>
				</Text>
			}
		>
			{results.length > 1 ? (
				<Box flexWrap="wrap" columnGap={1} marginBottom={1}>
					{results.map((result, index) => {
						const count = tallies[index]!.count;
						const mark = result.error ? glyph.fail : count > 0 ? glyph.checked : glyph.unchecked;
						const state = result.error ? (result.cancelled ? '已取消' : '失败') : formatCount(count);
						const label = ` ${mark} ${truncateMiddle(baseName(result.path), 20)}  ${state} `;
						if (index === active) {
							return (
								<Text key={result.root} backgroundColor={color.accent} color="#1A1B26" bold>
									{label}
								</Text>
							);
						}
						return (
							<Text key={result.root} color={result.error && !result.cancelled ? color.danger : count > 0 ? undefined : color.subtle}>
								{label}
							</Text>
						);
					})}
				</Box>
			) : null}

			{current ? (
				<Group label={truncateMiddle(current.path, Math.min(48, Math.floor(columns / 2)))} right={meta}>
					{current.error ? (
						<Notice tone={current.cancelled ? 'warn' : 'danger'}>
							{current.cancelled ? '扫描被取消了，这个位置没有结果' : current.error}
						</Notice>
					) : current.count === 0 ? (
						<Box paddingY={1}>
							<Notice tone="ok">这里很干净，没有需要处理的内容</Notice>
						</Box>
					) : (
						<>
							<Text color={color.subtle} wrap="truncate">
								{currentOff
									? '整个位置已跳过，不会做任何改动。按 a 恢复。'
									: flatten
										? `${glyph.checked} 的每一行：左边文件夹里的文件移到右边，移完后变空的中间文件夹会删掉。`
										: `${glyph.checked} 的文件夹会被删除（连同里面的隐藏文件、desktop.ini 等）。`}
							</Text>
							<Box flexDirection="column" height={listHeight}>
								{visible.map((row, index) => {
									const at = offset + index === cursor;
									const on = row.state === 'on' && !currentOff;
									return (
										<Box key={row.index}>
											<Text color={color.accent}>{at ? `${glyph.pointer} ` : '  '}</Text>
											<Text color={on ? color.accent : color.faint}>{on ? glyph.checked : glyph.unchecked} </Text>
											<ItemLine root={current.path} row={currentOff ? {...row, state: 'skipped'} : row} width={listWidth} />
											<Text color={color.faint}>
												{currentOff ? '' : row.state === 'skipped' ? '  跳过' : row.state === 'blocked' ? '  保留·里面有跳过的' : ''}
											</Text>
										</Box>
									);
								})}
							</Box>
							<Text color={color.subtle}>
								{'    '}
								{formatCount(Math.min(cursor + 1, rows.length))} / {formatCount(rows.length)}
								{hidden > 0
									? `  ${glyph.dot}  共 ${formatCount(current.count)} 项，这里只列出前 ${formatCount(current.items.length)} 项，按 o 查看全部`
									: ''}
								{current.ignoredFiles > 0 ? `  ${glyph.dot}  连带清理最多 ${formatCount(current.ignoredFiles)} 个隐藏 / 噪音文件` : ''}
							</Text>
						</>
					)}
				</Group>
			) : null}
		</Frame>
	);
}
