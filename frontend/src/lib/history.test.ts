import { afterEach, describe, expect, it, vi } from "vitest";
import { memoryStorage, message } from "./fixtures";
import { HISTORY_VERSION, History } from "./history.svelte";

afterEach(() => {
	vi.useRealTimers();
});

describe("History", () => {
	it("appends messages and tracks the highest seq", () => {
		const history = new History({ maxMessages: 10 });
		expect(history.append(message({ seq: 3 }))).toBe(true);
		expect(history.append(message({ seq: 5 }))).toBe(true);
		expect(history.length).toBe(2);
		expect(history.lastSeq).toBe(5);
	});

	it("drops duplicates by id", () => {
		const history = new History({ maxMessages: 10 });
		history.append(message({ seq: 1, id: "same" }));
		expect(history.append(message({ seq: 1, id: "same" }))).toBe(false);
		expect(history.length).toBe(1);
	});

	it("ignores messages from another channel", () => {
		const history = new History({ maxMessages: 10 });
		history.channel = "channel";
		expect(history.append(message({ channel: "other" }))).toBe(false);
		expect(history.length).toBe(0);
	});

	it("removes a message on delete", () => {
		const history = new History({ maxMessages: 10 });
		history.append(message({ seq: 1, id: "m1" }));
		history.append(message({ seq: 2, id: "m2" }));
		history.append(message({ seq: 3, kind: "delete", id: "m1" }));
		expect(history.entries.map((entry) => entry.message.id)).toEqual(["m2"]);
	});

	it("removes only the messages of one user on clear_user", () => {
		const history = new History({ maxMessages: 10 });
		history.append(
			message({ seq: 1, id: "a", user: { ...message().user, login: "a" } }),
		);
		history.append(
			message({ seq: 2, id: "b", user: { ...message().user, login: "b" } }),
		);
		history.append(
			message({
				seq: 3,
				kind: "clear_user",
				user: { ...message().user, login: "a" },
			}),
		);
		expect(history.entries.map((entry) => entry.message.id)).toEqual(["b"]);
	});

	it("clears everything on clear", () => {
		const history = new History({ maxMessages: 10 });
		history.append(message({ seq: 1 }));
		history.append(message({ seq: 2, kind: "clear" }));
		expect(history.length).toBe(0);
	});

	it("keeps at most maxMessages entries", () => {
		const history = new History({ maxMessages: 2 });
		history.append(message({ seq: 1 }));
		history.append(message({ seq: 2 }));
		history.append(message({ seq: 3 }));
		expect(history.entries.map((entry) => entry.message.seq)).toEqual([2, 3]);
	});

	it("hides events when showEvents is off but remembers them", () => {
		const history = new History({ maxMessages: 10, showEvents: false });
		expect(history.append(message({ seq: 1, kind: "event", id: "e1" }))).toBe(
			false,
		);
		expect(history.length).toBe(0);
		expect(history.append(message({ seq: 1, kind: "event", id: "e1" }))).toBe(
			false,
		);
	});

	it("routes local notices to the status and keeps them out of the feed", () => {
		const notices: string[] = [];
		const history = new History({
			maxMessages: 10,
			onNotice: (entry) => notices.push(entry.text),
		});
		expect(
			history.append(
				message({
					seq: 1,
					kind: "notice",
					id: "local-2026-11-05T01:35:51.118Z",
					text: "Connected to chat",
				}),
			),
		).toBe(false);
		expect(notices).toEqual(["Connected to chat"]);
		expect(history.length).toBe(0);
		expect(
			history.append(
				message({
					seq: 2,
					kind: "notice",
					id: "3f6ab8d0-1e0e-4b56-9d60-1a2b3c4d5e6f",
				}),
			),
		).toBe(true);
		expect(history.length).toBe(1);
	});

	it("persists entries and restores them", () => {
		const storage = memoryStorage();
		vi.useFakeTimers();
		const history = new History({ maxMessages: 10, storage });
		history.channel = "channel";
		history.append(message({ seq: 1 }));

		const saved = JSON.parse(
			storage.data.get("twitch-chat:history:channel") ?? "null",
		);
		expect(saved).toBeNull();

		vi.advanceTimersByTime(500);
		const page = JSON.parse(
			storage.data.get("twitch-chat:history:channel") ?? "null",
		);
		expect(page.version).toBe(HISTORY_VERSION);
		expect(page.lastSeq).toBe(1);
		expect(page.items).toHaveLength(1);

		const restored = new History({ maxMessages: 10, storage });
		restored.useChannel("channel");
		expect(restored.entries.map((entry) => entry.message.seq)).toEqual([1]);
		expect(restored.lastSeq).toBe(1);
	});

	it("ignores saved history from another version", () => {
		const storage = memoryStorage();
		storage.setItem(
			"twitch-chat:history:channel",
			JSON.stringify({ version: 1, lastSeq: 9, items: [message({ seq: 9 })] }),
		);
		const history = new History({ maxMessages: 10, storage });
		history.useChannel("channel");
		expect(history.length).toBe(0);
		expect(history.lastSeq).toBe(0);
	});

	it("does not persist the overlay", () => {
		vi.useFakeTimers();
		const history = new History({ maxMessages: 10, storage: null });
		history.channel = "channel";
		history.append(message({ seq: 1 }));
		vi.advanceTimersByTime(1000);
		expect(history.length).toBe(1);
	});

	it("trims entries when maxMessages shrinks", () => {
		const history = new History({ maxMessages: 10 });
		history.append(message({ seq: 1 }));
		history.append(message({ seq: 2 }));
		history.append(message({ seq: 3 }));
		history.setMaxMessages(1);
		expect(history.entries.map((entry) => entry.message.seq)).toEqual([3]);
		history.append(message({ seq: 4 }));
		expect(history.entries.map((entry) => entry.message.seq)).toEqual([4]);
	});

	it("keeps history of another channel when switching", () => {
		const storage = memoryStorage();
		vi.useFakeTimers();
		const history = new History({ maxMessages: 10, storage });
		history.useChannel("one");
		history.append(message({ seq: 1, channel: "one" }));
		history.flush();
		history.useChannel("two");
		expect(history.length).toBe(0);
		history.useChannel("one");
		expect(history.entries.map((entry) => entry.message.seq)).toEqual([1]);
	});
});
