// 纯逻辑的单元测试：粘贴路径的拆分、确认页每一行的状态。
import assert from 'node:assert/strict';
import {test} from 'node:test';
import type {RootScan} from '../engine/protocol.js';
import {buildRows, itemPath, parsePaths} from '../state.js';

test('parsePaths：单个带空格的路径保持完整', () => {
	assert.deepEqual(parsePaths('D:\\My Photos\\2025'), ['D:\\My Photos\\2025']);
	assert.deepEqual(parsePaths('  "D:\\My Photos"  '), ['D:\\My Photos']);
});

test('parsePaths：拖入多个文件夹时按路径拆开', () => {
	assert.deepEqual(parsePaths('D:\\one D:\\two'), ['D:\\one', 'D:\\two']);
	assert.deepEqual(parsePaths('D:\\my one E:\\my two'), ['D:\\my one', 'E:\\my two']);
	assert.deepEqual(parsePaths('"C:\\有 空格" D:\\无空格'), ['C:\\有 空格', 'D:\\无空格']);
	assert.deepEqual(parsePaths('D:\\a \\\\server\\share'), ['D:\\a', '\\\\server\\share']);
	assert.deepEqual(parsePaths('D:\\a\r\nD:\\b c\n'), ['D:\\a', 'D:\\b c']);
});

function emptyScan(paths: string[]): RootScan {
	return {
		root: 0,
		path: 'C:\\root',
		error: null,
		cancelled: false,
		stats: null,
		elapsedMs: 0,
		count: paths.length,
		ignoredFiles: 0,
		fileCount: 0,
		items: paths.map(path => ({path})),
	};
}

test('buildRows：跳过一个文件夹，它里面的保留，上级标记为删不掉', () => {
	const scan = emptyScan(['C:\\root\\a\\b\\c', 'C:\\root\\a\\b', 'C:\\root\\a', 'C:\\root\\ab']);
	const states = Object.fromEntries(buildRows(scan, new Set([1])).map(row => [itemPath(row.item), row.state]));
	assert.deepEqual(states, {
		'C:\\root\\a': 'blocked',
		'C:\\root\\a\\b': 'skipped',
		'C:\\root\\a\\b\\c': 'skipped',
		'C:\\root\\ab': 'on',
	});
});

test('buildRows：大小写和引擎一样按原样比较（区分大小写的文件夹里 A 和 a 是两个）', () => {
	const scan = emptyScan(['C:\\root\\A\\x', 'C:\\root\\a\\x', 'C:\\root\\A', 'C:\\root\\a']);
	const states = Object.fromEntries(buildRows(scan, new Set([2])).map(row => [itemPath(row.item), row.state]));
	assert.deepEqual(states, {
		'C:\\root\\A': 'skipped',
		'C:\\root\\A\\x': 'skipped',
		'C:\\root\\a': 'on',
		'C:\\root\\a\\x': 'on',
	});
});
