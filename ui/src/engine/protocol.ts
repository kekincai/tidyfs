// 与 Rust 端 src/app/serve/protocol.rs 保持一致。

export type TaskKind = 'empty' | 'flatten';
export type FlattenMode = 'keep-endpoints' | 'one-level' | 'collapse-chain';

export type DriveKind = 'fixed' | 'removable' | 'network' | 'cdrom' | 'ramdisk' | 'unknown';

export interface Drive {
	path: string;
	label: string;
	fileSystem: string;
	kind: DriveKind;
	totalBytes: number;
	freeBytes: number;
}

export interface Stats {
	dirs: number;
	files: number;
	skipped: number;
	errors: number;
}

export type Item = {path: string} | {from: string; to: string; files: number};

export interface RootScan {
	root: number;
	path: string;
	error: string | null;
	cancelled: boolean;
	stats: Stats | null;
	elapsedMs: number;
	count: number;
	ignoredFiles: number;
	fileCount: number;
	items: Item[];
}

export interface FailureItem {
	action: string;
	path: string;
	error: string;
}

export interface RootApply {
	root: number;
	path: string;
	moved: number;
	removedDirs: number;
	removedFiles: number;
	failureCount: number;
	failures: FailureItem[];
	journal: string | null;
	cancelled: boolean;
	elapsedMs: number;
}

export interface Hello {
	version: string;
	flattenMode: FlattenMode;
	logDir: string;
	journalDir: string;
	configPath: string | null;
}

type Body =
	| ({type: 'hello'} & Hello)
	| {type: 'drives'; drives: Drive[]}
	| {type: 'picked'; paths: string[]}
	| {type: 'scan-started'; roots: string[]}
	| {type: 'scan-progress'; root: number; stats: Stats; elapsedMs: number}
	| ({type: 'scan-root'} & RootScan)
	| {type: 'scan-done'; cancelled: boolean}
	| {type: 'apply-started'; roots: number[]}
	| {type: 'apply-progress'; root: number; current: number; total: number}
	| ({type: 'apply-root'} & RootApply)
	| {type: 'apply-done'; cancelled: boolean}
	| {type: 'exported'; path: string}
	| {type: 'ok'}
	| {type: 'error'; message: string};

export type Message = Body & {id: number; final: boolean};
export type MessageOf<T extends Message['type']> = Extract<Message, {type: T}>;

export type Request =
	| {cmd: 'hello'}
	| {cmd: 'drives'}
	| {cmd: 'pick'}
	| {cmd: 'scan'; task: TaskKind; mode?: FlattenMode; roots: string[]}
	| {cmd: 'apply'; scan: number; roots: number[]}
	| {cmd: 'export'; scan: number; root: number}
	| {cmd: 'cancel'; job: number}
	| {cmd: 'forget'; scan: number};
