<script lang="ts">
	import Button from "@ku6epxboctuk/ui-kit/components/Button.svelte";
	import type { PanelChat } from "../chat.svelte";

	let { chat }: { chat: PanelChat } = $props();

	let view = $derived.by(() => {
		if (chat.stream.offline) {
			return { state: "bad", label: "Server is offline", retry: true };
		}
		if (!chat.authenticated) {
			return { state: "bad", label: "Sign in to Twitch", retry: false };
		}
		const connected = chat.stream.linkState === "connected";
		return {
			state: connected ? "ok" : "warn",
			label: "",
			retry: !connected,
		};
	});
</script>

<header class="status" data-state={view.state}>
	<span class="status-dot"></span>
	<span class="status-label">{view.label || chat.notice}</span>
	{#if view.retry}
		<div class="retry">
			<Button variant="ghost" size="sm" onclick={() => chat.stream.reconnect()}
				>Reconnect</Button
			>
		</div>
	{/if}
</header>
