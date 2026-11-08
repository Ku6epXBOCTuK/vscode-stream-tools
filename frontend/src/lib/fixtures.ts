import type { ChatMessage, User } from "./types";

export type MemoryStorage = {
	data: Map<string, string>;
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
};

export function memoryStorage(): MemoryStorage {
	const data = new Map<string, string>();
	return {
		data,
		getItem: (key) => data.get(key) ?? null,
		setItem: (key, value) => {
			data.set(key, value);
		},
	};
}

const USER: User = {
	id: "u1",
	login: "viewer",
	name: "Viewer",
	color: "#ff0000",
	badges: [],
	moderator: false,
	subscriber: false,
	vip: false,
	broadcaster: false,
};

export function message(
	overrides: Partial<ChatMessage> & { seq?: number } = {},
): ChatMessage {
	const seq = overrides.seq ?? 1;
	return {
		seq,
		id: `m${seq}`,
		channel: "channel",
		ts: "2026-11-04T12:00:00Z",
		kind: "chat",
		user: USER,
		text: `text ${seq}`,
		fragments: [{ type: "text", text: `text ${seq}` }],
		action: false,
		highlighted: false,
		moderatable: false,
		...overrides,
	};
}

export function streamEvent(payload: unknown): { data: string } {
	return { data: JSON.stringify(payload) };
}
