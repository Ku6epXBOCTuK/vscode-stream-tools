import type { Chatter } from "../lib/types";

export type Mention = {
	start: number;
	query: string;
};

export function mentionAt(text: string, caret: number): Mention | null {
	const match = /(?:^|\s)@([a-zA-Z0-9_]*)$/.exec(text.slice(0, caret));
	if (!match) {
		return null;
	}
	return { start: caret - match[1].length - 1, query: match[1] };
}

export function filterChatters(
	list: Chatter[],
	query: string,
	limit = 8,
): Chatter[] {
	const needle = query.toLowerCase();
	return list
		.filter(
			(chatter) =>
				chatter.login.toLowerCase().startsWith(needle) ||
				chatter.name.toLowerCase().startsWith(needle),
		)
		.slice(0, limit);
}

export function insertMention(
	text: string,
	caret: number,
	start: number,
	name: string,
): { text: string; caret: number } {
	const before = text.slice(0, start);
	const rest = text.slice(caret);
	const after = rest.startsWith(" ") ? rest.slice(1) : rest;
	return {
		text: `${before}@${name} ${after}`,
		caret: start + name.length + 2,
	};
}
