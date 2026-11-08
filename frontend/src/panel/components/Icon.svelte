<script lang="ts">
	let { name }: { name: "reply" | "delete" | "timeout" | "ban" | "close" } =
		$props();

	const paths = {
		reply: "M6.4 3.6 3 7l3.4 3.4M3 7h6.2a4 4 0 0 1 4 4v1.2",
		delete: "M3.2 4.8h9.6M6.4 4.8V3.4h3.2v1.4M4.7 4.8l.6 7.4h5.4l.6-7.4",
		timeout: "M8 5.2v3l2.1 1.3",
		ban: "M4.7 11.3 11.3 4.7",
		close: "M4.5 4.5l7 7M11.5 4.5l-7 7",
	};

	let circled = $derived(name === "timeout" || name === "ban");
</script>

<svg
	viewBox="0 0 16 16"
	fill="none"
	stroke="currentColor"
	stroke-width="1.4"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
>
	{#if circled}
		<circle cx="8" cy="8" r="5.2"></circle>
	{/if}
	<path d={paths[name]}></path>
</svg>
