import { describe, expect, it, vi } from "vitest";
import type { Api } from "./api";
import { memoryStorage, message, streamEvent } from "./fixtures";
import { History } from "./history.svelte";
import { SILENCE_MS, type SourceLike, Stream } from "./stream.svelte";
import type { State } from "./types";

type Handler = (event: { data?: string }) => void;

function fakeSource() {
	const handlers = new Map<string, Handler>();
	const source: SourceLike = {
		close: vi.fn(),
		addEventListener: (type, handler) => {
			handlers.set(type, handler);
		},
	};
	return {
		source,
		emit: (type: string, payload?: unknown) => {
			const handler = handlers.get(type);
			if (!handler) {
				throw new Error(`no handler for ${type}`);
			}
			handler(payload === undefined ? {} : streamEvent(payload));
		},
	};
}

function state(overrides: Partial<State> = {}): State {
	return {
		channel: "channel",
		port: 45890,
		authenticated: true,
		login: "viewer",
		widget: {
			max_messages: 12,
			fade_seconds: 45,
			font_size: 22,
			show_badges: true,
		},
		eventsub: "connected",
		messages: 1,
		...overrides,
	};
}

function fakeApi(page: { current: State } = { current: state() }) {
	const messages = vi.fn(async () => ({
		items: [message({ seq: 1 })],
		last_seq: 1,
	}));
	const api = {
		messages,
		state: vi.fn(async () => page.current),
		send: vi.fn(),
		badges: vi.fn(),
		remove: vi.fn(),
		timeout: vi.fn(),
		ban: vi.fn(),
	} as unknown as Api;
	return { api, messages, page };
}

function build(options: { history?: History; api?: Api } = {}) {
	const history =
		options.history ??
		new History({ maxMessages: 50, storage: memoryStorage() });
	const { api, messages } = fakeApi();
	const clock = { now: 1_000 };
	const sources: ReturnType<typeof fakeSource>[] = [];
	const stream = new Stream({
		api: options.api ?? api,
		history,
		now: () => clock.now,
		source: () => {
			const fake = fakeSource();
			sources.push(fake);
			return fake.source;
		},
		onError: vi.fn(),
	});
	return { history, stream, messages, clock, sources };
}

describe("Stream", () => {
	it("adopts a server seq that fell behind and catches up from zero", async () => {
		const { history, stream, messages, sources } = build();
		history.append(message({ seq: 5 }));
		stream.connect();
		sources[0].emit("status", { eventsub: "connected", last_seq: 0 });
		expect(history.lastSeq).toBe(0);
		await vi.waitFor(() => expect(messages).toHaveBeenCalledWith(0));
	});

	it("keeps the history when the server seq is ahead", () => {
		const { history, stream, sources } = build();
		history.append(message({ seq: 5 }));
		stream.connect();
		sources[0].emit("status", { eventsub: "connected", last_seq: 9 });
		expect(history.lastSeq).toBe(5);
	});

	it("appends messages that arrive on the stream", () => {
		const { history, stream, sources } = build();
		stream.connect();
		sources[0].emit("message", message({ seq: 7, id: "seven" }));
		expect(history.entries.map((entry) => entry.message.id)).toEqual(["seven"]);
	});

	it("reports the link state from the status event", () => {
		const { stream, sources } = build();
		stream.connect();
		sources[0].emit("status", { eventsub: "offline", last_seq: 0 });
		expect(stream.linkState).toBe("offline");
	});

	it("reconnects after the silence timeout", () => {
		const { stream, clock, sources } = build();
		stream.connect();
		expect(sources).toHaveLength(1);
		clock.now += SILENCE_MS - 1;
		expect(stream.watch()).toBe(false);
		clock.now += 1;
		expect(stream.watch()).toBe(true);
		expect(sources).toHaveLength(2);
		expect(sources[0].source.close).toHaveBeenCalled();
	});

	it("switches the history to a new channel on poll", async () => {
		const history = new History({ maxMessages: 50, storage: memoryStorage() });
		history.useChannel("one");
		history.append(message({ seq: 1, channel: "one" }));
		const page = { current: state({ channel: "two" }) };
		const { api, messages } = fakeApi(page);
		const stream = new Stream({ api, history });

		await stream.poll();
		expect(history.channel).toBe("two");
		expect(history.length).toBe(0);
		await vi.waitFor(() => expect(messages).toHaveBeenCalled());
	});

	it("catches up from the last known seq when the server is ahead", async () => {
		const history = new History({ maxMessages: 50 });
		const page = { current: state({ messages: 9 }) };
		const { api, messages } = fakeApi(page);
		const stream = new Stream({ api, history });
		history.useChannel("channel");
		history.append(message({ seq: 3 }));

		await stream.poll();
		expect(history.lastSeq).toBe(3);
		await vi.waitFor(() => expect(messages).toHaveBeenCalledWith(3));
	});

	it("marks itself offline when the state request fails", async () => {
		const api = {
			state: vi.fn(async () => {
				throw new Error("down");
			}),
		} as unknown as Api;
		const stream = new Stream({
			api,
			history: new History({ maxMessages: 5 }),
		});
		expect(await stream.refresh()).toBe(false);
		expect(stream.offline).toBe(true);
	});
});
