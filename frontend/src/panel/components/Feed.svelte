<script lang="ts">
	import Button from "@ku6epxboctuk/ui-kit/components/Button.svelte";
	import type { PanelChat } from "../chat.svelte";
	import Message from "./Message.svelte";

	let { chat }: { chat: PanelChat } = $props();

	let feed = $state<HTMLElement | null>(null);
	let follow = $state(true);
	let jumping = false;

	$effect(() => {
		chat.history.entries.length;
		if (follow && feed) {
			feed.scrollTop = feed.scrollHeight;
		}
	});

	function gap(): number {
		if (!feed) {
			return 0;
		}
		return feed.scrollHeight - feed.scrollTop - feed.clientHeight;
	}

	function syncFollow(): void {
		if (jumping) {
			if (gap() > 1) {
				return;
			}
			jumping = false;
		}
		follow = gap() <= 40;
	}

	function jump(): void {
		if (!feed) {
			return;
		}
		jumping = true;
		follow = true;
		feed.scrollTo({ top: feed.scrollHeight, behavior: "smooth" });
		setTimeout(() => {
			jumping = false;
			syncFollow();
		}, 900);
	}
</script>

<div class="feed-wrap">
	<main
		class="feed"
		bind:this={feed}
		onscroll={syncFollow}
		role="log"
		aria-live="polite"
	>
		{#each chat.history.entries as entry (entry.key)}
			<Message message={entry.message} {chat} />
		{/each}
	</main>
	{#if !follow}
		<div class="jump">
			<Button variant="secondary" size="sm" onclick={jump}
				>Jump to latest</Button
			>
		</div>
	{/if}
</div>
