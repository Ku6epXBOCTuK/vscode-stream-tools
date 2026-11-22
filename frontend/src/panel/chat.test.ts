import { afterEach, describe, expect, it, vi } from "vitest";
import { PanelChat } from "./chat.svelte";

type SendableChat = {
	api: {
		send: (text: string, replyTo?: string) => Promise<unknown>;
	};
};

function makeChat(): PanelChat {
	vi.stubGlobal("location", { search: "" });
	vi.stubGlobal("localStorage", {
		getItem: () => null,
		setItem: () => undefined,
	});
	const chat = new PanelChat();
	chat.stream.catchUp = () => undefined;
	return chat;
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("PanelChat.send", () => {
	it("keeps the pending state until the request resolves", async () => {
		const chat = makeChat();
		let finish = (): void => undefined;
		(chat as unknown as SendableChat).api.send = () =>
			new Promise((resolve) => {
				finish = () => resolve(undefined);
			});

		const pending = chat.send("hello");
		await Promise.resolve();
		expect(chat.sending).toBe(true);

		finish();
		expect(await pending).toBe(true);
		expect(chat.sending).toBe(false);
	});

	it("clears the pending state when the request fails", async () => {
		const chat = makeChat();
		(chat as unknown as SendableChat).api.send = () =>
			Promise.reject(new Error("offline"));

		expect(await chat.send("hello")).toBe(false);
		expect(chat.sending).toBe(false);
		expect(chat.error).toBe("offline");
	});

	it("ignores another send while one is in flight", async () => {
		const chat = makeChat();
		let finish = (): void => undefined;
		const send = vi.fn(() =>
			new Promise((resolve) => {
				finish = () => resolve(undefined);
			}),
		);
		(chat as unknown as SendableChat).api.send = send;

		const pending = chat.send("hello");
		expect(await chat.send("again")).toBe(false);
		expect(send).toHaveBeenCalledTimes(1);

		finish();
		expect(await pending).toBe(true);
	});
});
