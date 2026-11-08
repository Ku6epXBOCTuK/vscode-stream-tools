<script lang="ts">
	import type { Chatter } from "../mentions";

	let {
		items,
		selected,
		onpick,
	}: {
		items: Chatter[];
		selected: number;
		onpick: (chatter: Chatter) => void;
	} = $props();
</script>

<div class="mentions" role="listbox" aria-label="Viewers">
	{#each items as chatter, index (chatter.login)}
		<button
			type="button"
			class="mention-item"
			class:active={index === selected}
			role="option"
			aria-selected={index === selected}
			onmousedown={(event) => event.preventDefault()}
			onclick={() => onpick(chatter)}
		>
			<span class="mention-name">{chatter.name}</span>
			<span class="mention-login">@{chatter.login}</span>
		</button>
	{/each}
</div>
