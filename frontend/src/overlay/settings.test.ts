import { describe, expect, it } from "vitest";
import type { Widget } from "../lib/types";
import { resolveOverlaySettings } from "./settings";

const widget: Widget = {
	max_messages: 30,
	fade_seconds: 90,
	font_size: 28,
	show_badges: true,
};

function params(query: string): URLSearchParams {
	return new URLSearchParams(query);
}

describe("resolveOverlaySettings", () => {
	it("falls back to the defaults without a widget", () => {
		expect(resolveOverlaySettings(params(""))).toEqual({
			maxMessages: 12,
			fadeSeconds: 0,
			fontSize: 22,
		});
	});

	it("takes the widget settings when the query is empty", () => {
		expect(resolveOverlaySettings(params(""), widget)).toEqual({
			maxMessages: 30,
			fadeSeconds: 90,
			fontSize: 28,
		});
	});

	it("lets the query win over the widget settings", () => {
		expect(
			resolveOverlaySettings(params("max=5&fade=10&font=40"), widget),
		).toEqual({
			maxMessages: 5,
			fadeSeconds: 10,
			fontSize: 40,
		});
	});

	it("keeps fade=0 to disable fading", () => {
		expect(resolveOverlaySettings(params("fade=0"), widget).fadeSeconds).toBe(
			0,
		);
	});

	it("ignores invalid and empty values", () => {
		expect(
			resolveOverlaySettings(params("max=abc&fade=&font=-4"), widget),
		).toEqual({
			maxMessages: 30,
			fadeSeconds: 90,
			fontSize: 28,
		});
	});
});
