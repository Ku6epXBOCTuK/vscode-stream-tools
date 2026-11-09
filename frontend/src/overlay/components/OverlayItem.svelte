<script lang="ts">
	import type { ChatMessage } from "../../lib/types";
	import type { OverlayChat } from "../chat.svelte";
	import { reveal } from "../reveal";
	import Badge from "./Badge.svelte";
	import Body from "./Body.svelte";

	let {
		message,
		chat,
		ongone,
	}: {
		message: ChatMessage;
		chat: OverlayChat;
		ongone: () => void;
	} = $props();

	let gone = $state(false);

	$effect(() => {
		if (chat.fadeSeconds <= 0) {
			return;
		}
		const hide = setTimeout(() => {
			gone = true;
		}, chat.fadeSeconds * 1000);
		const drop = setTimeout(ongone, chat.fadeSeconds * 1000 + 700);
		return () => {
			clearTimeout(hide);
			clearTimeout(drop);
		};
	});
</script>

<li class="row" class:gone use:reveal>
	<span class="inner">
		<span class="inner-padding">
			{#if chat.showBadges}
				<span class="badges">
					{#each message.user.badges as badge (badge.name + badge.version)}
						<Badge
							{badge}
							url={chat.badges[`${badge.name}/${badge.version}`]}
						/>
					{/each}
				</span>
			{/if}
			<span class="username" style:color={message.user.color ?? undefined}
				>{message.user.name}</span
			>
			<span
				class="text-message"
				class:reply={message.reply !== undefined}
				class:highlighted={message.highlighted}
			>
				<Body fragments={message.fragments} />
			</span>
		</span>
	</span>
</li>
