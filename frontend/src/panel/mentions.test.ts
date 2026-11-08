import { describe, expect, it } from "vitest";
import { message } from "../lib/fixtures";
import type { Entry } from "../lib/history.svelte";
import {
	chattersFrom,
	filterChatters,
	insertMention,
	mentionAt,
} from "./mentions";

function entries(logins: string[]): Entry[] {
	return logins.map((login, index) => {
		const msg = message({ seq: index + 1, id: `m${index + 1}` });
		msg.user = { ...msg.user, login, name: login.toUpperCase() };
		return { key: `m${index + 1}`, message: msg };
	});
}

describe("mentions", () => {
	it("lists chatters by recency and without duplicates", () => {
		const list = chattersFrom(entries(["alice", "bob", "alice"]));
		expect(list.map((chatter) => chatter.login)).toEqual(["alice", "bob"]);
	});

	it("stops at the limit", () => {
		expect(chattersFrom(entries(["a", "b", "c"]), 2)).toHaveLength(2);
	});

	it("finds a mention at the caret", () => {
		expect(mentionAt("hi @ali", 7)).toEqual({ start: 3, query: "ali" });
		expect(mentionAt("@", 1)).toEqual({ start: 0, query: "" });
		expect(mentionAt("hi @ali", 2)).toBeNull();
		expect(mentionAt("mail@ali", 8)).toBeNull();
	});

	it("filters by login or display name prefix", () => {
		const list = [
			{ login: "alice", name: "Alice" },
			{ login: "bob", name: "Bob" },
		];
		expect(filterChatters(list, "al").map((c) => c.login)).toEqual(["alice"]);
		expect(filterChatters(list, "BO").map((c) => c.login)).toEqual(["bob"]);
		expect(filterChatters(list, "")).toHaveLength(2);
	});

	it("inserts the mention and leaves the caret after the space", () => {
		expect(insertMention("hi @ali there", 7, 3, "Alice")).toEqual({
			text: "hi @Alice there",
			caret: 10,
		});
	});

	it("keeps the display name out of the login casing", () => {
		const list = [{ login: "ku6epxboctuk", name: "Ku6epXBOCTuK" }];
		const [chatter] = filterChatters(list, "ku6");
		expect(insertMention("@", 1, 0, chatter.name)).toEqual({
			text: "@Ku6epXBOCTuK ",
			caret: 14,
		});
	});
});
