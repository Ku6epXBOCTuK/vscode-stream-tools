import type { Api } from "./api";
import type { History } from "./history.svelte";
import type { ChatMessage, LinkState, State } from "./types";

export const POLL_MS = 10_000;
export const WATCHDOG_MS = 5_000;
export const SILENCE_MS = 20_000;

export type StreamEvent = { data?: string };

export type SourceLike = {
	close(): void;
	addEventListener(type: string, handler: (event: StreamEvent) => void): void;
};

export type SourceFactory = (url: string) => SourceLike;

type Options = {
	api: Api;
	history: History;
	source?: SourceFactory;
	now?: () => number;
	onError?: (text: string) => void;
};

export class Stream {
	state = $state<State | null>(null);
	linkState = $state<LinkState>("connecting");
	lastEventAt = $state(0);
	offline = $state(false);

	#api: Api;
	#history: History;
	#source: SourceFactory;
	#now: () => number;
	#onError: (text: string) => void;
	#stream: SourceLike | null = null;
	#timers: ReturnType<typeof setInterval>[] = [];

	constructor(options: Options) {
		this.#api = options.api;
		this.#history = options.history;
		this.#source =
			options.source ?? ((url) => new EventSource(url) as SourceLike);
		this.#now = options.now ?? (() => Date.now());
		this.#onError = options.onError ?? (() => undefined);
		this.lastEventAt = this.#now();
	}

	get authenticated(): boolean {
		return this.state?.authenticated ?? false;
	}

	get channel(): string {
		return this.state?.channel ?? "";
	}

	secondsSinceEvent(): number {
		return Math.max(0, Math.round((this.#now() - this.lastEventAt) / 1000));
	}

	async start(): Promise<void> {
		await this.refresh();
		this.#history.useChannel(this.channel);
		this.catchUp();
		this.connect();
		this.#timers.push(
			setInterval(() => this.watch(), WATCHDOG_MS),
			setInterval(() => void this.poll(), POLL_MS),
		);
	}

	stop(): void {
		this.#stream?.close();
		this.#stream = null;
		for (const timer of this.#timers) {
			clearInterval(timer);
		}
		this.#timers = [];
	}

	connect(): void {
		this.#stream?.close();
		this.lastEventAt = this.#now();
		const stream = this.#source("/chat/events");
		this.#stream = stream;
		stream.addEventListener("open", () => {
			this.lastEventAt = this.#now();
			this.catchUp();
		});
		stream.addEventListener("status", (event) => {
			this.lastEventAt = this.#now();
			const payload = this.#parse(event);
			if (!payload) {
				return;
			}
			this.#setLink(payload.eventsub);
			this.adopt(payload.last_seq);
		});
		stream.addEventListener("ping", () => {
			this.lastEventAt = this.#now();
		});
		stream.addEventListener("message", (event) => {
			this.lastEventAt = this.#now();
			const message = this.#parse(event);
			if (message) {
				this.#history.append(message as unknown as ChatMessage);
			}
		});
		stream.addEventListener("error", () => {
			this.linkState = "connecting";
		});
	}

	watch(): boolean {
		if (this.#now() - this.lastEventAt < SILENCE_MS) {
			return false;
		}
		this.lastEventAt = this.#now();
		this.linkState = "connecting";
		this.connect();
		return true;
	}

	adopt(serverSeq: unknown): boolean {
		if (typeof serverSeq !== "number" || serverSeq >= this.#history.lastSeq) {
			return false;
		}
		this.#history.lastSeq = 0;
		this.#history.flush();
		this.catchUp();
		return true;
	}

	catchUp(): void {
		this.#api
			.messages(this.#history.lastSeq)
			.then((page) => {
				for (const message of page.items) {
					this.#history.append(message);
				}
			})
			.catch((error: Error) => {
				this.offline = true;
				this.#onError(`History is unavailable: ${error.message}`);
			});
	}

	async refresh(): Promise<boolean> {
		try {
			const state = await this.#api.state();
			this.state = state;
			this.offline = false;
			this.linkState = state.eventsub;
			return true;
		} catch {
			this.offline = true;
			return false;
		}
	}

	async poll(): Promise<void> {
		let state: State;
		try {
			state = await this.#api.state();
		} catch {
			this.offline = true;
			return;
		}
		this.state = state;
		this.offline = false;
		this.linkState = state.eventsub;
		if (state.channel !== this.#history.channel) {
			this.#history.useChannel(state.channel);
			this.catchUp();
			return;
		}
		const behind =
			typeof state.messages === "number" &&
			state.messages > this.#history.lastSeq;
		this.adopt(state.messages);
		if (behind) {
			this.catchUp();
		}
	}

	reconnect(): void {
		this.connect();
		this.catchUp();
	}

	#setLink(value: unknown): void {
		if (
			value === "connected" ||
			value === "offline" ||
			value === "connecting"
		) {
			this.linkState = value;
		}
	}

	#parse(event: StreamEvent): Record<string, unknown> | null {
		if (!event.data) {
			return null;
		}
		try {
			return JSON.parse(event.data) as Record<string, unknown>;
		} catch {
			return null;
		}
	}
}
