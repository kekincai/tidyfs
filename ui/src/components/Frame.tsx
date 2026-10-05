import {Box, Text, useWindowSize} from 'ink';
import type {ReactNode} from 'react';
import {KeyHints, Rule, type Hint} from './primitives.js';
import {STEPS, color, glyph, type Step} from './theme.js';

/**
 * 全屏框架：
 *   ◆ tidyfs   选择位置 › 扫描 › 确认 › 执行 › 完成          右侧状态
 *   ───────────────────────────────────────────────────────────
 *   内容
 *   ───────────────────────────────────────────────────────────
 *   按键提示
 */
export function Frame({step, status, hints, children}: {step: Step; status?: ReactNode; hints: Hint[]; children: ReactNode}) {
	const {columns, rows} = useWindowSize();
	const inner = Math.max(20, columns - 4);
	const current = STEPS.indexOf(step);

	return (
		<Box flexDirection="column" width={columns} height={rows} paddingX={2} paddingTop={1}>
			<Box justifyContent="space-between">
				<Box columnGap={3}>
					<Text>
						<Text color={color.accent}>{glyph.brand}</Text>
						<Text bold> tidyfs</Text>
					</Text>
					{columns < 72 ? (
						<Text color={color.accent} bold>
							{step}
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
					<Text color={color.subtle} wrap="truncate">
						{status}
					</Text>
				) : null}
			</Box>
			<Rule width={inner} />
			<Box flexDirection="column" flexGrow={1} overflow="hidden" paddingTop={1}>
				{children}
			</Box>
			<Rule width={inner} />
			<KeyHints hints={hints} />
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
