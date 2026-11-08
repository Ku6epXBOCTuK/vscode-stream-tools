<script lang="ts">
	import IconButton from "@ku6epxboctuk/ui-kit/components/IconButton.svelte";
	import { formatTime } from "../../lib/fragments";
	import type { ChatMessage } from "../../lib/types";
	import type { PanelChat } from "../chat.svelte";
	import Badge from "./Badge.svelte";
	import Body from "./Body.svelte";
	import Icon from "./Icon.svelte";

	let { message, chat }: { message: ChatMessage; chat: PanelChat } = $props();
</script>

<article
	class="message {message.kind}"
	class:highlight={message.highlighted}
	class:action={message.action}
	class:has-actions={message.moderatable}
>
	<div class="meta">
		{#if chat.showBadges}
			<span class="badges">
				{#each message.user.badges as badge (badge.name + badge.version)}
					<Badge {badge} url={chat.badges[`${badge.name}/${badge.version}`]} />
				{/each}
			</span>
		{/if}
		<span class="name" style:color={message.user.color}
			>{message.user.name}</span
		>
		<span class="time" title={message.ts}>{formatTime(message.ts)}</span>
	</div>
	{#if message.reply}
		<div class="reply-quote">
			<strong>{message.reply.user}:</strong>
			{message.reply.text}
		</div>
	{/if}
	<Body fragments={message.fragments} />
	{#if message.moderatable}
		<div class="actions">
			<IconButton
				label="Reply"
				size="sm"
				title="Reply"
				onclick={() => chat.reply(message)}
			>
				<Icon name="reply" />
			</IconButton>
			<IconButton
				label="Delete"
				variant="danger"
				size="sm"
				title="Delete"
				onclick={() => chat.moderate("delete", message)}
			>
				<Icon name="delete" />
			</IconButton>
			<IconButton
				label="Timeout"
				variant="danger"
				size="sm"
				title="Timeout"
				onclick={() => chat.moderate("timeout", message)}
			>
				<Icon name="timeout" />
			</IconButton>
			<IconButton
				label="Ban"
				variant="danger"
				size="sm"
				title="Ban"
				onclick={() => chat.moderate("ban", message)}
			>
				<Icon name="ban" />
			</IconButton>
		</div>
	{/if}
</article>
