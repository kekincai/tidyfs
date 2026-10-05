import {Box, Text, useInput, useWindowSize} from 'ink';
import {useEffect, useRef, useState} from 'react';
import type {EngineClient} from '../engine/client.js';
import type {RootApply, RootScan} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Callout, Meter, Notice, PathText, Spinner} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {formatCount} from '../lib/format.js';
import {TASK_LABEL, type Settings} from '../state.js';

interface Progress {
	current: number;
	total: number;
	result: RootApply | null;
}

interface Props {
	engine: EngineClient;
	settings: Settings;
	scanId: number;
	results: RootScan[];
	roots: number[];
	onDone: (applied: RootApply[], cancelled: boolean) => void;
	onBack: () => void;
}

export function Applying({engine, settings, scanId, results, roots, onDone, onBack}: Props) {
	const {columns} = useWindowSize();
	const [progress, setProgress] = useState<Record<number, Progress>>({});
	const [error, setError] = useState<string | null>(null);
	const [cancelling, setCancelling] = useState(false);
	const job = useRef<number | null>(null);

	useEffect(() => {
		const applied: RootApply[] = [];
		job.current = engine.stream({cmd: 'apply', scan: scanId, roots}, message => {
			switch (message.type) {
				case 'apply-progress':
					setProgress(previous => ({
						...previous,
						[message.root]: {current: message.current, total: message.total, result: previous[message.root]?.result ?? null},
					}));
					break;
				case 'apply-root': {
					const {type: _type, id: _id, final: _final, ...result} = message;
					applied.push(result);
					setProgress(previous => {
						const total = previous[result.root]?.total ?? 1;
						return {...previous, [result.root]: {current: total, total, result}};
					});
					break;
				}
				case 'apply-done':
					onDone(
						applied.sort((a, b) => a.root - b.root),
						message.cancelled,
					);
					break;
				case 'error':
					setError(message.message);
					break;
			}
		});
	}, []);

	useInput((_char, key) => {
		if (error) {
			if (key.return || key.escape) onBack();
			return;
		}
		if (key.escape && job.current !== null && !cancelling) {
			setCancelling(true);
			engine.cancel(job.current);
		}
	});

	const pathWidth = Math.max(8, Math.min(36, columns - 56));
	const meterWidth = Math.max(6, Math.min(36, columns - pathWidth - 34));
	const totals = Object.values(progress).reduce((sum, item) => ({current: sum.current + item.current, total: sum.total + item.total}), {
		current: 0,
		total: 0,
	});
	const percent = totals.total > 0 ? Math.floor((totals.current / totals.total) * 100) : 0;

	return (
		<Frame
			step="执行"
			status={`${TASK_LABEL[settings.task]} ${glyph.dot} ${percent}%`}
			hints={error ? [['Enter', '返回']] : [['Esc', cancelling ? '正在停止…' : '停止（已完成的操作不会撤销）']]}
		>
			<Group label="执行中" right={`${roots.length} 个位置 · 不同磁盘并行`}>
				{roots.map(root => {
					const scan = results.find(result => result.root === root);
					const state = progress[root];
					const done = state?.result;
					const failed = (done?.failureCount ?? 0) > 0;
					return (
						<Box key={root} columnGap={2}>
							<Box width={1}>
								{done ? <Text color={failed ? color.warn : color.ok}>{failed ? glyph.warn : glyph.ok}</Text> : <Spinner />}
							</Box>
							<PathText path={scan?.path ?? String(root)} width={pathWidth} strong />
							<Meter
								ratio={done ? 1 : state ? state.current / Math.max(1, state.total) : 0}
								width={meterWidth}
								tint={failed ? color.warn : done ? color.ok : color.accent}
							/>
							<Text color={color.subtle}>{state ? `${formatCount(state.current)} / ${formatCount(state.total)}` : '等待中'}</Text>
						</Box>
					);
				})}
			</Group>
			{cancelling ? <Notice tone="warn">当前这一步完成后停止…</Notice> : null}
			{error ? (
				<Callout tone="danger" title="执行没有开始">
					{<Text>{error}</Text>}
				</Callout>
			) : null}
		</Frame>
	);
}
