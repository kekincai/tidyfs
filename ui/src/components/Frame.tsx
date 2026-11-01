import {Box, Text, useWindowSize} from 'ink';
import type {ReactNode} from 'react';
import {KeyHints, Rule, type Hint} from './primitives.js';
import {STEPS, color, glyph, type Step} from './theme.js';

/**
 * 全屏框架：
 *   ◆ tidyfs   选择位置 › 扫描 › 确认 › 执行 › 完成          右侧状态（窄窗口只显示当前步骤）
 *   ───────────────────────────────────────────────────────────
 *   内容（比窗口高时裁掉底部）
 *   固定区：输入框 / 确认框 / 提示
 *   ───────────────────────────────────────────────────────────
 *   按键提示
 */
export function Frame({
	step,
	status,
	hints,
	footer,
	children,
}: {
	step: Step;
	status?: ReactNode;
	hints: Hint[];
	/** 固定在按键提示上方、永远不会被裁掉的区域：输入框、确认框、提示信息。 */
	footer?: ReactNode;
	children: ReactNode;
}) {
	const {columns, rows} = useWindowSize();
	const inner = Math.max(20, columns - 4);
	const current = STEPS.indexOf(step);

	return (
		<Box flexDirection="column" width={columns} height={rows} paddingX={2} paddingTop={1}>
			<Box justifyContent="space-between" flexShrink={0}>
				<Box columnGap={3} flexShrink={0}>
					<Text>
						<Text color={color.accent}>{glyph.brand}</Text>
						<Text bold> tidyfs</Text>
					</Text>
					{columns < 100 ? (
						<Text>
							<Text color={color.accent} bold>
								{step}
							</Text>
							<Text color={color.faint}>
								{' '}
								{current + 1}/{STEPS.length}
							</Text>
						</Text>
					) : (
						<Text>
							{STEPS.map((name, index) => (
								<Text key={name}>
									{index > 0 ? <Text color={color.faint}> {glyph.pointer} </Text> : null}
									<Text color={index === current ? color.accent : index < current ? color.subtle : color.faint} bold={index === current}>
										{name}
									</Text>
								</Text>
							))}
						</Text>
					)}
				</Box>
				{status && columns >= 60 ? (
					<Box flexShrink={1} marginLeft={2} overflow="hidden">
						<Text color={color.subtle} wrap="truncate-end">
							{status}
						</Text>
					</Box>
				) : null}
			</Box>
			<Box flexShrink={0}>
				<Rule width={inner} />
			</Box>
			{/* 内容比窗口高时只裁掉内容区，标题和按键提示始终可见。 */}
			<Box flexDirection="column" flexGrow={1} flexShrink={1} overflow="hidden" paddingTop={1}>
				<Box flexDirection="column" flexShrink={0}>
					{children}
				</Box>
			</Box>
			{footer ? (
				<Box flexDirection="column" flexShrink={0}>
					{footer}
				</Box>
			) : null}
			<Box flexDirection="column" flexShrink={0}>
				<Rule width={inner} />
				<KeyHints hints={hints} />
			</Box>
		</Box>
	);
}

/** 内容区里的一个分组：标题分隔线 + 内容。 */
export function Group({label, right, children}: {label: string; right?: string; children: ReactNode}) {
	const {columns} = useWindowSize();
	return (
		<Box flexDirection="column" marginBottom={1}>
			<Rule width={Math.max(20, columns - 4)} label={label} right={right} />
			<Box flexDirection="column" paddingTop={0}>
				{children}
			</Box>
		</Box>
	);
}
