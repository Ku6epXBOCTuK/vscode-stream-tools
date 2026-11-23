import { messageKey } from "./fragments";
import type { Chatter, ChatMessage } from "./types";

export const HISTORY_VERSION = 3;
export const CHATTER_LIMIT = 50;

export type Entry = {
	key: string;
	message: ChatMessage;
};

export type StorageLike = {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
};

type Options = {
	maxMessages: number;
	showEvents?: boolean;
	storage?: StorageLike | null;
	keyPrefix?: string;
	saveDelay?: number;
	onNotice?: (message: ChatMessage) => void;
};

function mentionableUser(message: ChatMessage): Chatter | null {
	if (!message.user.id.trim()) {
		return null;
	}
	return {
		login: message.user.login,
		name: message.user.name || message.user.login,
	};
}

export class History {
	entries = $state<Entry[]>([]);
	chatters = $state<Chatter[]>([]);
	lastSeq = $state(0);
	channel = $state("");

	#maxMessages: number;
	#showEvents: boolean;
	#storage: StorageLike | null;
	#keyPrefix: string;
	#saveDelay: number;
	#saveTimer: ReturnType<typeof setTimeout> | null = null;
	#seen = new Set<string>();
	#restoring = false;
	#onNotice?: (message: ChatMessage) => void;

	constructor(options: Options) {
		this.#maxMessages = options.maxMessages;
		this.#showEvents = options.showEvents ?? true;
		this.#storage = options.storage ?? null;
		this.#keyPrefix = options.keyPrefix ?? "twitch-chat:history:";
		this.#saveDelay = options.saveDelay ?? 500;
		this.#onNotice = options.onNotice;
	}

	get length(): number {
		return this.entries.length;
	}

	append(message: ChatMessage): boolean {
		if (message.channel && this.channel && message.channel !== this.channel) {
			return false;
		}
		this.lastSeq = Math.max(this.lastSeq, message.seq);

		if (message.kind === "delete") {
			const target = this.entries.find(
				(entry) => entry.message.id === message.id,
			);
			if (target) {
				this.forget(target);
			}
			return false;
		}
		if (message.kind === "clear") {
			this.clear();
			return false;
		}
		if (message.kind === "clear_user") {
			this.#remember(message.user.login, "");
			for (const entry of [...this.entries]) {
				if (entry.message.user.login === message.user.login) {
					this.forget(entry);
				}
			}
			return false;
		}

		const key = messageKey(message);
		if (this.#seen.has(key)) {
			return false;
		}
		this.#seen.add(key);
		if (
			message.kind === "notice" &&
			(!message.id || message.id.startsWith("local-"))
		) {
			this.#onNotice?.(message);
			return false;
		}
		if (!this.#restoring) {
			const chatter = mentionableUser(message);
			if (chatter) {
				this.#remember(chatter.login, chatter.name);
			}
		}
		if (message.kind === "event" && !this.#showEvents) {
			return false;
		}

		this.entries.push({ key, message });
		while (this.entries.length > this.#maxMessages) {
			this.forget(this.entries[0]);
		}
		this.save();
		return true;
	}

	clear(): void {
		this.entries = [];
		this.#seen.clear();
		this.save();
	}

	forget(entry: Entry): void {
		this.entries = this.entries.filter((item) => item !== entry);
		this.#seen.delete(entry.key);
		this.save();
	}

	setMaxMessages(next: number): void {
		if (!Number.isFinite(next) || next <= 0 || next === this.#maxMessages) {
			return;
		}
		this.#maxMessages = next;
		while (this.entries.length > this.#maxMessages) {
			this.forget(this.entries[0]);
		}
	}

	useChannel(next: string): void {
		this.channel = next;
		this.entries = [];
		this.chatters = [];
		this.#seen.clear();
		this.lastSeq = 0;
		this.restore();
	}

	restore(): void {
		if (!this.#storage) {
			return;
		}
		let saved: unknown = null;
		try {
			saved = JSON.parse(this.#storage.getItem(this.#key()) ?? "null");
		} catch {
			return;
		}
		const page = saved as {
			version?: number;
			lastSeq?: number;
			items?: ChatMessage[];
			chatters?: Chatter[];
		};
		if (
			!page ||
			!Array.isArray(page.items) ||
			(page.version !== HISTORY_VERSION &&
				page.version !== HISTORY_VERSION - 1)
		) {
			return;
		}
		this.lastSeq = Number(page.lastSeq) || 0;
		const legacy = !Array.isArray(page.chatters);
		if (!legacy) {
			for (const chatter of page.chatters ?? []) {
				this.#remember(chatter.login, chatter.name);
			}
		}
		this.#restoring = !legacy;
		try {
			for (const message of page.items) {
				this.append(message);
			}
		} finally {
			this.#restoring = false;
		}
	}

	save(): void {
		if (!this.#storage) {
			return;
		}
		this.#cancelSave();
		this.#saveTimer = setTimeout(() => this.flush(), this.#saveDelay);
	}

	flush(): void {
		if (!this.#storage) {
			return;
		}
		this.#cancelSave();
		try {
			this.#storage.setItem(
				this.#key(),
				JSON.stringify({
					version: HISTORY_VERSION,
					lastSeq: this.lastSeq,
					items: this.entries.map((entry) => entry.message),
					chatters: this.chatters.map((chatter) => ({
						login: chatter.login,
						name: chatter.name,
					})),
				}),
			);
		} catch {
			return;
		}
	}

	#remember(login: string, name: string): void {
		if (!login) {
			return;
		}
		const index = this.chatters.findIndex((chatter) => chatter.login === login);
		if (index === -1) {
			this.chatters.unshift({ login, name: name || login });
		} else {
			const [chatter] = this.chatters.splice(index, 1);
			this.chatters.unshift({ login, name: name || chatter.name || login });
		}
		while (this.chatters.length > CHATTER_LIMIT) {
			this.chatters.pop();
		}
	}

	#key(): string {
		return `${this.#keyPrefix}${this.channel || "none"}`;
	}

	#cancelSave(): void {
		if (this.#saveTimer !== null) {
			clearTimeout(this.#saveTimer);
			this.#saveTimer = null;
		}
	}
}
