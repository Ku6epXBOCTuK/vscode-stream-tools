import { createApi } from "./api";
import { History } from "./history.svelte";
import { Stream } from "./stream.svelte";

export type ChatCoreOptions = {
	maxMessages: number;
	showEvents: boolean;
	storage: Storage | null;
};

export class ChatCore {
	history: History;
	stream: Stream;
	badges = $state<Record<string, string>>({});
	showBadges = $state(true);
	error = $state("");
	notice = $state("");
	protected api = createApi();

	constructor(options: ChatCoreOptions) {
		this.history = new History({
			maxMessages: options.maxMessages,
			showEvents: options.showEvents,
			storage: options.storage,
			onNotice: (message) => {
				this.notice = message.text;
			},
		});
		this.stream = new Stream({
			api: this.api,
			history: this.history,
			onError: (text) => {
				this.error = text;
			},
		});
	}

	get authenticated(): boolean {
		return this.stream.authenticated;
	}

	async start(): Promise<void> {
		await this.stream.start();
		this.applyWidget();
		await this.loadBadges();
	}

	applyWidget(): void {
		const widget = this.stream.state?.widget;
		if (widget) {
			this.showBadges = widget.show_badges !== false;
		}
	}

	stop(): void {
		this.history.flush();
		this.stream.stop();
	}

	async loadBadges(): Promise<void> {
		try {
			const page = await this.api.badges();
			this.badges = page.badges;
		} catch {
			this.badges = {};
		}
	}
}
