import { ChatCore } from "../lib/chat.svelte";
import { resolveOverlaySettings } from "./settings";

export class OverlayChat extends ChatCore {
	fadeSeconds = $state(45);
	fontSize = $state(22);
	#params: URLSearchParams;

	constructor() {
		const params = new URLSearchParams(location.search);
		const settings = resolveOverlaySettings(params);
		super({
			maxMessages: settings.maxMessages,
			showEvents: params.get("events") !== "0",
			storage: null,
		});
		this.#params = params;
		this.fadeSeconds = settings.fadeSeconds;
		this.fontSize = settings.fontSize;
	}

	override applyWidget(): void {
		super.applyWidget();
		const settings = resolveOverlaySettings(
			this.#params,
			this.stream.state?.widget,
		);
		this.history.setMaxMessages(settings.maxMessages);
		this.fadeSeconds = settings.fadeSeconds;
		this.fontSize = settings.fontSize;
	}
}
