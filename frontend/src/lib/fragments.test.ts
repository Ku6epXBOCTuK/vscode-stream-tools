import { describe, expect, it } from "vitest";
import { message } from "./fixtures";
import { emoteUrl, formatTime, messageKey } from "./fragments";

describe("fragments", () => {
	it("builds the 2.0 emote url", () => {
		expect(emoteUrl("25")).toBe(
			"https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0",
		);
	});

	it("keys a message by id or by its local seq", () => {
		expect(messageKey(message({ seq: 4, id: "abc" }))).toBe("abc");
		expect(messageKey(message({ seq: 4, id: "", ts: "t" }))).toBe("local:t:4");
	});

	it("formats the timestamp as hours and minutes", () => {
		const time = formatTime("2026-11-04T12:34:00Z");
		expect(time).toMatch(/^\d{2}:\d{2}$/);
	});
});
