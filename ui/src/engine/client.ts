import {spawn, type ChildProcessWithoutNullStreams} from 'node:child_process';
import {createInterface} from 'node:readline';
import type {Logger} from 'pino';
import type {Message, MessageOf, Request} from './protocol.js';

type Listener = (message: Message) => void;

export class EngineError extends Error {}

/**
 * 常驻的 `tidyfs serve` 子进程。请求通过 stdin 发送，响应按 id 分发；
 * 一个请求的最后一条消息带 `final: true`。
 */
export class EngineClient {
	private readonly child: ChildProcessWithoutNullStreams;
	private readonly listeners = new Map<number, Listener>();
	private nextId = 1;
	private exited = false;

	constructor(
		enginePath: string,
		private readonly log: Logger,
	) {
		this.child = spawn(enginePath, ['serve'], {windowsHide: true});
		this.log.info({enginePath, pid: this.child.pid}, 'engine started');

		createInterface({input: this.child.stdout}).on('line', line => {
			let message: Message;
			try {
				message = JSON.parse(line) as Message;
			} catch {
				this.log.warn({line}, 'invalid engine output');
				return;
			}
			if (message.type === 'error') {
				this.log.warn({id: message.id, message: message.message}, 'engine error');
			}
			const listener = this.listeners.get(message.id);
			if (message.final) {
				this.listeners.delete(message.id);
			}
			listener?.(message);
		});

		this.child.stderr.on('data', (chunk: Buffer) => {
			this.log.warn({stderr: chunk.toString('utf8').trim()}, 'engine stderr');
		});

		const onGone = (reason: string) => {
			if (this.exited) return;
			this.exited = true;
			this.log.error({reason}, 'engine exited');
			for (const [id, listener] of this.listeners) {
				listener({id, final: true, type: 'error', message: `引擎已退出：${reason}`});
			}
			this.listeners.clear();
		};
		this.child.on('error', error => onGone(error.message));
		// 引擎提前退出时写 stdin 会 EPIPE，交给 exit 事件统一处理。
		this.child.stdin.on('error', error => this.log.warn({err: error}, 'engine stdin error'));
		this.child.on('exit', code => onGone(`退出码 ${code ?? '未知'}`));
	}

	/** 发送请求，每条响应都会回调；返回请求 id（也是可取消的任务 id）。 */
	stream(request: Request, onMessage: Listener): number {
		const id = this.nextId++;
		if (this.exited) {
			queueMicrotask(() => onMessage({id, final: true, type: 'error', message: '引擎未运行'}));
			return id;
		}
		this.listeners.set(id, onMessage);
		this.log.debug({id, request}, 'request');
		this.child.stdin.write(JSON.stringify({id, ...request}) + '\n');
		return id;
	}

	/** 只关心最终结果的请求。 */
	async request<T extends Message['type']>(request: Request, expected: T): Promise<MessageOf<T>> {
		return new Promise((resolve, reject) => {
			this.stream(request, message => {
				if (!message.final) return;
				if (message.type === expected) {
					resolve(message as MessageOf<T>);
				} else if (message.type === 'error') {
					reject(new EngineError(message.message));
				} else {
					reject(new EngineError(`意外的响应：${message.type}`));
				}
			});
		});
	}

	cancel(job: number): void {
		this.stream({cmd: 'cancel', job}, () => {});
	}

	/** 关闭 stdin，引擎会取消正在进行的任务并安全退出。 */
	close(): void {
		if (!this.exited) this.child.stdin.end();
	}
}
