import { describe, expect, it } from "vitest";
import { REVEAL_MS_PER_PX, revealDuration } from "./reveal";

describe("revealDuration", () => {
	it("is 0.6ms per pixel", () => {
		expect(REVEAL_MS_PER_PX).toBe(0.6);
		expect(revealDuration(100)).toBe(60);
		expect(revealDuration(1000)).toBe(600);
	});

	it("rounds to whole milliseconds", () => {
		expect(revealDuration(101)).toBe(61);
	});

	it("is zero for empty and broken widths", () => {
		expect(revealDuration(0)).toBe(0);
		expect(revealDuration(-10)).toBe(0);
		expect(revealDuration(Number.NaN)).toBe(0);
		expect(revealDuration(Number.POSITIVE_INFINITY)).toBe(0);
	});
});
