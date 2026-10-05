import {Box, Text, useInput, useWindowSize} from 'ink';
import type {Hello, RootApply} from '../engine/protocol.js';
import {Frame, Group} from '../components/Frame.js';
import {Notice, PathText, Stat} from '../components/primitives.js';
import {color, glyph} from '../components/theme.js';
import {formatCount, formatDuration, openExternal, truncateMiddle} from '../lib/format.js';
import {TASK_LABEL, type Settings} from '../state.js';

interface Props {
	settings: Settings;
	hello: Hello | null;
	applied: RootApply[];
	cancelled: boolean;
	onBack: () => void;
	onQuit: () => void;
}

const FAILURE_PREVIEW = 6;

export function Summary({settings, hello, applied, cancelled, onBack, onQuit}: Props) {
	const {columns} = useWindowSize();
	const sum = (pick: (result: RootApply) => number) => applied.reduce((total, result) => total + pick(result), 0);
	const failures = applied.flatMap(result => result.failures);
	const failureCount = sum(result => result.failureCount);
	const elapsed = Math.max(0, ...applied.map(result => result.elapsedMs));

	useInput((char, key) => {
		if (key.return || key.escape) return onBack();
		if (char === 'q') return onQuit();
		if (char === 'j' && hello) return openExternal(hello.journalDir);
		if (char === 'l' && hello) return openExternal(hello.logDir);
	});

	const headline = cancelled ? '已停止' : failureCount > 0 ? '完成，有部分项目没处理成功' : '全部完成';
	const headlineColor = cancelled || failureCount > 0 ? color.warn : color.ok;

	return (
		<Frame
			step="完成"
			status={TASK_LABEL[settings.task]}
			hints={[
				['Enter', '回到开始'],
				['j', '打开操作日志'],
				['l', '打开诊断日志'],
				['q', '退出'],
			]}
		>
			<Text color={headlineColor} bold>
				{cancelled || failureCount > 0 ? glyph.warn : glyph.ok} {headline}
			</Text>

			<Box columnGap={4} marginY={1} paddingLeft={2}>
				<Stat value={formatCount(sum(result => result.removedDirs))} label="删除的文件夹" />
				{settings.task === 'flatten' ? <Stat value={formatCount(sum(result => result.moved))} label="移动的文件" /> : null}
				<Stat value={formatCount(sum(result => result.removedFiles))} label="清理的隐藏 / 噪音文件" tint={color.violet} />
				<Stat value={formatCount(failureCount)} label="失败" tint={failureCount ? color.danger : color.subtle} />
				<Stat value={formatDuration(elapsed)} label="用时" tint={color.subtle} />
			</Box>

			<Group label="各位置">
				{applied.map(result => {
					const failed = result.failureCount > 0;
					return (
						<Box key={result.root} columnGap={2}>
							<Text color={failed ? color.warn : color.ok}>{failed ? glyph.warn : glyph.ok}</Text>
							<PathText path={result.path} width={Math.min(36, columns - 50)} strong />
							<Text color={color.subtle}>
								{[
									`删除 ${formatCount(result.removedDirs)} 个文件夹`,
									result.moved ? `移动 ${formatCount(result.moved)} 个文件` : '',
									failed ? `失败 ${formatCount(result.failureCount)}` : '',
									result.cancelled ? '已停止' : '',
								]
									.filter(Boolean)
									.join(`  ${glyph.dot}  `)}
							</Text>
						</Box>
					);
				})}
			</Group>

			{failures.length > 0 ? (
				<Group
					label="没处理成功的"
					right={failureCount > FAILURE_PREVIEW ? `仅显示前 ${FAILURE_PREVIEW} 项，完整内容见操作日志` : undefined}
				>
					{failures.slice(0, FAILURE_PREVIEW).map((failure, index) => (
						<Text key={index} wrap="truncate">
							<Text color={color.danger}>{glyph.fail} </Text>
							{truncateMiddle(failure.path, Math.floor((columns - 8) * 0.55))}
							<Text color={color.subtle}> {failure.error}</Text>
						</Text>
					))}
				</Group>
			) : null}

			{settings.task === 'empty' ? <Notice tone="info">如果资源管理器正停在已删除的文件夹里，回到上层目录或按 F5 刷新即可。</Notice> : null}
		</Frame>
	);
}
