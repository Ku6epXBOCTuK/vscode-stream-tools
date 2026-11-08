<script lang="ts">
	import { emoteUrl } from "../../lib/fragments";
	import type { Fragment } from "../../lib/types";

	let { fragments }: { fragments: Fragment[] } = $props();
</script>

<div class="body">
	{#each fragments as fragment, index (index)}
		{#if fragment.type === "emote"}
			<img
				class="emote"
				src={emoteUrl(fragment.id)}
				alt={fragment.text}
				loading="lazy"
			>
		{:else if fragment.type === "cheermote"}
			<span class="cheer" data-tier={fragment.tier}>{fragment.text}</span>
		{:else if fragment.type === "mention"}
			<span class="mention">{fragment.text}</span>
		{:else}
			<span class="text">{fragment.text}</span>
		{/if}
	{/each}
</div>
