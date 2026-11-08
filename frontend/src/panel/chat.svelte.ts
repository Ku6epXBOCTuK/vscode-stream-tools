import { ChatCore } from "../lib/chat.svelte";
import type { ChatMessage } from "../lib/types";

export type Moderation = "delete" | "timeout" | "ban";

export class PanelChat extends ChatCore {
	replyTo = $state<ChatMessage | null>(null);

	constructor() {
		const params = new URLSearchParams(location.search);
		super({
			maxMessages: Number(params.get("max")) || 500,
			showEvents: params.get("events") !== "0",
			storage: localStorage,
		});
	}

	reply(message: ChatMessage): void {
		this.replyTo = message;
	}

	cancelReply(): void {
		this.replyTo = null;
	}

	dismissError(): void {
		this.error = "";
	}

	async send(text: string): Promise<boolean> {
		const trimmed = text.trim();
		if (!trimmed) {
			return false;
		}
		try {
			await this.api.send(trimmed, this.replyTo?.id);
			this.replyTo = null;
			this.error = "";
			this.stream.catchUp();
			return true;
		} catch (error) {
			this.error = (error as Error).message;
			return false;
		}
	}

	async moderate(action: Moderation, message: ChatMessage): Promise<void> {
		try {
			if (action === "delete") {
				await this.api.remove(message.id);
				return;
			}
			const reason = window.prompt("Reason (optional)", "") ?? "";
			if (action === "timeout") {
				const minutes = Number(window.prompt("Timeout in minutes", "10"));
				if (!minutes || minutes <= 0) {
					return;
				}
				await this.api.timeout(
					message.user.id,
					Math.round(minutes * 60),
					reason,
				);
				return;
			}
			await this.api.ban(message.user.id, reason);
		} catch (error) {
			this.error = (error as Error).message;
		}
	}
}
