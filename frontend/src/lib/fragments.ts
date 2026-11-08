import type { ChatMessage } from "./types";

const EMOTE_BASE = "https://static-cdn.jtvnw.net/emoticons/v2";

export function emoteUrl(id: string): string {
	return `${EMOTE_BASE}/${id}/default/dark/2.0`;
}

export function messageKey(message: ChatMessage): string {
	return message.id || `local:${message.ts}:${message.seq}`;
}

export function formatTime(ts: string): string {
	return new Date(ts).toLocaleTimeString([], {
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	});
}
