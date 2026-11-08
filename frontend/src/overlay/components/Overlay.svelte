<script lang="ts">
	import { SvelteSet } from "svelte/reactivity";
	import { OverlayChat } from "../chat.svelte";
	import OverlayItem from "./OverlayItem.svelte";

	const chat = new OverlayChat();
	const hidden = new SvelteSet<string>();

	$effect(() => {
		void chat.start();
		const flush = () => chat.history.flush();
		window.addEventListener("pagehide", flush);
		return () => {
			window.removeEventListener("pagehide", flush);
			chat.stop();
		};
	});
</script>

<div id="widget" style:font-size={`${chat.fontSize / 18}em`}>
	<div id="chat-messages">
		<ul role="log" aria-live="polite">
			{#each chat.history.entries as entry (entry.key)}
				{#if !hidden.has(entry.key)}
					<OverlayItem
						message={entry.message}
						{chat}
						ongone={() => hidden.add(entry.key)}
					/>
				{/if}
			{/each}
		</ul>
	</div>
	<div id="background"></div>
</div>
