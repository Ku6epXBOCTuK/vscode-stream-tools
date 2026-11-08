<script lang="ts">
	import type { Badge as BadgeData } from "../../lib/types";

	let { badge, url }: { badge: BadgeData; url?: string } = $props();
	let failed = $state(false);
	let title = $derived(`${badge.name} ${badge.version}`);
</script>

{#if url && !failed}
	<img
		class="badge"
		src={url}
		alt={badge.name}
		{title}
		loading="lazy"
		onerror={() => {
			failed = true;
		}}
	>
{:else}
	<span class="badge-text" data-badge={badge.name} {title}>
		{badge.name.slice(0, 1).toUpperCase()}
	</span>
{/if}
