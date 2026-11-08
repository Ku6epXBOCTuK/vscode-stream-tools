<script lang="ts">
	import Button from "@ku6epxboctuk/ui-kit/components/Button.svelte";
	import IconButton from "@ku6epxboctuk/ui-kit/components/IconButton.svelte";
	import TextInput from "@ku6epxboctuk/ui-kit/components/TextInput.svelte";
	import { tick } from "svelte";
	import type { PanelChat } from "../chat.svelte";
	import {
		type Chatter,
		chattersFrom,
		filterChatters,
		insertMention,
		mentionAt,
	} from "../mentions";
	import Icon from "./Icon.svelte";
	import MentionMenu from "./MentionMenu.svelte";

	let { chat }: { chat: PanelChat } = $props();

	let text = $state("");
	let caret = $state(0);
	let selected = $state(0);
	let dismissed = $state(false);
	let input = $state<HTMLInputElement | null>(null);

	let chatters = $derived(chattersFrom(chat.history.entries));
	let mention = $derived(dismissed ? null : mentionAt(text, caret));
	let candidates = $derived(
		mention ? filterChatters(chatters, mention.query) : [],
	);
	let menuOpen = $derived(mention !== null && candidates.length > 0);

	function typing(event: Event): void {
		input = event.currentTarget as HTMLInputElement;
		caret = input.selectionStart ?? text.length;
		selected = 0;
		dismissed = false;
	}

	function caretOnly(event: Event): void {
		input = event.currentTarget as HTMLInputElement;
		caret = input.selectionStart ?? text.length;
	}

	async function pick(chatter: Chatter): Promise<void> {
		if (!mention) {
			return;
		}
		const next = insertMention(text, caret, mention.start, chatter.name);
		text = next.text;
		dismissed = true;
		await tick();
		input?.focus();
		input?.setSelectionRange(next.caret, next.caret);
		caret = next.caret;
	}

	function onkeydown(event: KeyboardEvent): void {
		if (!menuOpen) {
			return;
		}
		if (event.key === "ArrowDown") {
			event.preventDefault();
			selected = (selected + 1) % candidates.length;
			return;
		}
		if (event.key === "ArrowUp") {
			event.preventDefault();
			selected = (selected - 1 + candidates.length) % candidates.length;
			return;
		}
		if (event.key === "Enter" || event.key === "Tab") {
			event.preventDefault();
			void pick(candidates[selected]);
			return;
		}
		if (event.key === "Escape") {
			event.preventDefault();
			dismissed = true;
		}
	}

	async function submit(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (!(await chat.send(text))) {
			return;
		}
		text = "";
		caret = 0;
	}
</script>

<form class="composer" onsubmit={submit}>
	{#if chat.replyTo}
		<div class="reply">
			<span class="reply-text">
				{chat.replyTo.user.name}: {chat.replyTo.text}
			</span>
			<IconButton
				label="Cancel reply"
				size="sm"
				title="Cancel reply"
				onclick={() => chat.cancelReply()}
			>
				<Icon name="close" />
			</IconButton>
		</div>
	{/if}
	<div class="composer-row">
		{#if menuOpen}
			<MentionMenu
				items={candidates}
				{selected}
				onpick={(chatter) => void pick(chatter)}
			/>
		{/if}
		<TextInput
			bind:value={text}
			size="sm"
			type="text"
			autocomplete="off"
			maxlength={500}
			placeholder="Send a message"
			oninput={typing}
			onclick={caretOnly}
			onkeyup={caretOnly}
			{onkeydown}
		/>
		<Button variant="primary" size="sm" type="submit">Send</Button>
	</div>
</form>
