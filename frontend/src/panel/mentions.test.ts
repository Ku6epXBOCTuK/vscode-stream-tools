import { describe, expect, it } from "vitest";
import { filterChatters, insertMention, mentionAt } from "./mentions";

describe("mentions", () => {
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
