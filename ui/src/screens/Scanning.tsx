import {Box, Text, useAnimation, useInput, useWindowSize} from 'ink';
import {useEffect, useRef, useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {RootScan, Stats} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Callout, Notice, PathText, Shimmer, Spinner} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {formatCount, formatDuration, padEnd} from '../lib/format.js';
import {TASK_LABEL, type Settings} from '../state.js';

interface Row {
	path: string;
	stats: Stats | null;
	elapsedMs: number;
	result: RootScan | null;
}

interface Props {
	engine: EngineClient;
	settings: Settings;
	roots: string[];
	onDone: (scanId: number, results: RootScan[]) => void;
	onBack: () => void;
}

const rate = (count: number, ms: number) => (ms > 300 ? `${formatCount(Math.round((count / ms) * 1000))}/s` : '');

export function Scanning({engine, settings, roots, onDone, onBack}: Props) {
	const {columns} = useWindowSize();
	const {time} = useAnimation({interval: 200});
	const [rows, setRows] = useState<Row[]>(() => roots.map(path => ({path, stats: null, elapsedMs: 0, result: null})));
	const [error, setError] = useState<string | null>(null);
	const [cancelling, setCancelling] = useState(false);
	const job = useRef<number | null>(null);

	useEffect(() => {
		const results: RootScan[] = [];
		job.current = engine.stream({cmd: 'scan', task: settings.task, mode: settings.mode, roots}, message => {
			switch (message.type) {
				case 'scan-started':
					setRows(message.roots.map(path => ({path, stats: null, elapsedMs: 0, result: null})));
					break;
				case 'scan-progress':
					setRows(previous =>
						previous.map((row, index) => (index === message.root ? {...row, stats: message.stats, elapsedMs: message.elapsedMs} : row)),
					);
					break;
				case 'scan-root': {
					const {type: _type, id: _id, final: _final, ...result} = message;
					results[result.root] = result;
					setRows(previous => previous.map((row, index) => (index === result.root ? {...row, result} : row)));
					break;
				}
				case 'scan-done':
					onDone(message.id, results.filter(Boolean));
					break;
				case 'error':
					setError(message.message);
					break;
			}
		});
	}, []);

	useInput((char, key) => {
		if (error) {
			if (key.return || key.escape) onBack();
			return;
		}
		if ((key.escape || char === 'q') && job.current !== null && !cancelling) {
			setCancelling(true);
			engine.cancel(job.current);
		}
	});

	const finished = rows.filter(row => row.result).length;
	const totalDirs = rows.reduce((sum, row) => sum + (row.result?.stats?.dirs ?? row.stats?.dirs ?? 0), 0);
	const wide = columns >= 100;
	const pathWidth = wide ? Math.max(12, Math.min(40, columns - 70)) : Math.max(8, Math.min(24, columns - 34));
	const unit = settings.task === 'empty' ? '个空文件夹' : '组可拉平';

	return (
		<Frame
			step="扫描"
			status={`${TASK_LABEL[settings.task]} ${glyph.dot} ${formatDuration(Math.round(time))}`}
			hints={error ? [['Enter', '返回']] : [['Esc', cancelling ? '正在取消…' : '取消']]}
		>
			<Group label="进度" right={`${finished}/${rows.length} 完成 · 共 ${formatCount(totalDirs)} 个目录`}>
				{rows.map(row => {
					const result = row.result;
					const stats = result?.stats ?? row.stats;
					const elapsed = result?.elapsedMs || row.elapsedMs;
					let icon = <Spinner />;
					let tail = stats ? <Shimmer width={12} /> : <Text color={color.faint}>等待同一磁盘上的任务</Text>;
					if (result?.error) {
						const tint = result.cancelled ? color.warn : color.danger;
						icon = <Text color={tint}>{result.cancelled ? glyph.stopped : glyph.fail}</Text>;
						tail = <Text color={tint}>{result.cancelled ? '已取消' : result.error}</Text>;
					} else if (result) {
						icon = <Text color={color.ok}>{glyph.ok}</Text>;
						tail =
							result.count > 0 ? (
								<Text>
									<Text color={color.accent} bold>
										{formatCount(result.count)}
									</Text>
									<Text color={color.subtle}> {unit}</Text>
								</Text>
							) : (
								<Text color={color.subtle}>没有需要处理的</Text>
							);
					}
					return (
						<Box key={row.path} columnGap={2}>
							<Box width={1}>{icon}</Box>
							<PathText path={row.path} width={pathWidth} strong />
							<Text color={color.subtle}>
								{padEnd(stats ? `${formatCount(stats.dirs)} 目录` : '', 14)}
								{wide ? padEnd(stats ? `${formatCount(stats.files)} 文件` : '', 16) : ''}
								{wide ? padEnd(stats ? formatDuration(elapsed) : '', 8) : ''}
								{wide ? padEnd(!result && stats ? rate(stats.dirs, elapsed) : '', 10) : ''}
							</Text>
							<Text wrap="truncate">{tail}</Text>
						</Box>
					);
				})}
			</Group>
			{cancelling ? <Notice tone="warn">正在停止，已经扫描完的位置会保留结果</Notice> : null}
			{error ? (
				<Callout tone="danger" title="扫描没有完成">
					{<Text>{error}</Text>}
				</Callout>
			) : null}
		</Frame>
	);
}
