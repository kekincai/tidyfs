import {Box, Text, useAnimation} from 'ink';
import stringWidth from 'string-width';
import type {ReactNode} from 'react';
import {truncateMiddle} from '../lib/format.js';
import {color, glyph} from './theme.js';

const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

export function Spinner({tint = color.accent}: {tint?: string}) {
	const {frame} = useAnimation({interval: 80});
	return <Text color={tint}>{SPINNER[frame % SPINNER.length]}</Text>;
}

/** 细线进度条：已完成部分用主色，剩余部分用很淡的轨道色。 */
export function Meter({ratio, width, tint = color.accent}: {ratio: number; width: number; tint?: string}) {
	const value = Math.max(0, Math.min(1, Number.isFinite(ratio) ? ratio : 0));
	const filled = Math.round(value * width);
	return (
		<Text>
			<Text color={tint}>{glyph.bar.repeat(filled)}</Text>
			<Text color={color.faint}>{glyph.bar.repeat(Math.max(0, width - filled))}</Text>
		</Text>
	);
}

/** 不确定进度时的流动光带。 */
export function Shimmer({width}: {width: number}) {
	const {frame} = useAnimation({interval: 60});
	const head = frame % (width + 6);
	return (
		<Text>
			{Array.from({length: width}, (_, index) => {
				const distance = head - index;
				const tint = distance >= 0 && distance < 2 ? color.accent : distance >= 2 && distance < 5 ? color.accentSoft : color.faint;
				return (
					<Text key={index} color={tint}>
						{glyph.bar}
					</Text>
				);
			})}
		</Text>
	);
}

/** 分隔线：`标题 ───────── 右侧说明`，铺满给定宽度。 */
export function Rule({width, label, right}: {width: number; label?: string; right?: string}) {
	const left = label ? `${label} ` : '';
	const tail = right ? ` ${right}` : '';
	const fill = Math.max(2, width - stringWidth(left) - stringWidth(tail));
	return (
		<Text>
			{label ? <Text bold>{left}</Text> : null}
			<Text color={color.faint}>{glyph.rule.repeat(fill)}</Text>
			{right ? <Text color={color.subtle}>{tail}</Text> : null}
		</Text>
	);
}

/** 路径：父目录淡色，最后一段正常色，读起来一眼能看到“是哪个文件夹”。 */
export function PathText({path, width, strong = false}: {path: string; width: number; strong?: boolean}) {
	const text = truncateMiddle(path, width);
	const cut = Math.max(text.lastIndexOf('\\'), text.lastIndexOf('/'));
	const isRootLike = cut === text.length - 1;
	const parent = cut >= 0 && !isRootLike ? text.slice(0, cut + 1) : '';
	const leaf = parent ? text.slice(cut + 1) : text;
	const pad = Math.max(0, width - stringWidth(text));
	return (
		<Text>
			<Text color={color.subtle}>{parent}</Text>
			<Text bold={strong}>{leaf}</Text>
			{' '.repeat(pad)}
		</Text>
	);
}

/** 分段选择器。 */
export function Segmented<T extends string>({options, value}: {options: {value: T; label: string}[]; value: T}) {
	return (
		<Box columnGap={1}>
			{options.map(option =>
				option.value === value ? (
					<Text key={option.value} backgroundColor={color.accent} color="#1A1B26" bold>
						{` ${option.label} `}
					</Text>
				) : (
					<Text key={option.value} color={color.subtle}>
						{` ${option.label} `}
					</Text>
				),
			)}
		</Box>
	);
}

export type Hint = [key: string, label: string];

export function KeyHints({hints}: {hints: Hint[]}) {
	return (
		<Box flexWrap="wrap" columnGap={3}>
			{hints.map(([key, label]) => (
				<Text key={key + label}>
					<Text color={color.accent}>{key}</Text>
					<Text color={color.subtle}> {label}</Text>
				</Text>
			))}
		</Box>
	);
}

export function Notice({tone, children}: {tone: 'ok' | 'warn' | 'danger' | 'info'; children: ReactNode}) {
	const tint = tone === 'info' ? color.subtle : color[tone];
	const icon = tone === 'ok' ? glyph.ok : tone === 'danger' ? glyph.fail : tone === 'warn' ? glyph.warn : glyph.dot;
	return (
		<Text>
			<Text color={tint}>{icon} </Text>
			<Text color={tone === 'info' ? color.subtle : undefined}>{children}</Text>
		</Text>
	);
}

/** 大数字 + 小标签的统计块。 */
export function Stat({value, label, tint = color.accent}: {value: string; label: string; tint?: string}) {
	return (
		<Box flexDirection="column" minWidth={14}>
			<Text color={tint} bold>
				{value}
			</Text>
			<Text color={color.subtle}>{label}</Text>
		</Box>
	);
}

/** 需要用户注意的浮层（确认、错误）。 */
export function Callout({tone, title, children}: {tone: 'warn' | 'danger' | 'accent'; title: string; children?: ReactNode}) {
	const tint = tone === 'accent' ? color.accent : color[tone];
	return (
		<Box borderStyle="round" borderColor={tint} paddingX={2} paddingY={0} flexDirection="column" marginTop={1}>
			<Text color={tint} bold>
				{title}
			</Text>
			{children}
		</Box>
	);
}
