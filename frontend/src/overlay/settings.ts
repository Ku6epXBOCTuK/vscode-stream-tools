import type { Widget } from "../lib/types";

export type OverlaySettings = {
	maxMessages: number;
	fadeSeconds: number;
	fontSize: number;
};

const DEFAULTS = {
	maxMessages: 12,
	fadeSeconds: 0,
	fontSize: 22,
};

export function resolveOverlaySettings(
	params: URLSearchParams,
	widget?: Widget,
): OverlaySettings {
	const max = positive(params.get("max")) ?? widget?.max_messages;
	const fade = nonNegative(params.get("fade")) ?? widget?.fade_seconds;
	const font =
		positive(params.get("font")) ??
		(widget && widget.font_size > 0 ? widget.font_size : undefined);

	return {
		maxMessages: max && max > 0 ? Math.round(max) : DEFAULTS.maxMessages,
		fadeSeconds: fade ?? DEFAULTS.fadeSeconds,
		fontSize: font && font > 0 ? Math.round(font) : DEFAULTS.fontSize,
	};
}

function positive(raw: string | null): number | undefined {
	const value = raw === null || raw.trim() === "" ? Number.NaN : Number(raw);
	return Number.isFinite(value) && value > 0 ? value : undefined;
}

function nonNegative(raw: string | null): number | undefined {
	const value = raw === null || raw.trim() === "" ? Number.NaN : Number(raw);
	return Number.isFinite(value) && value >= 0 ? value : undefined;
}
