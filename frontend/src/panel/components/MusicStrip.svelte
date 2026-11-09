<script lang="ts">
	import IconButton from "@ku6epxboctuk/ui-kit/components/IconButton.svelte";
	import IconAudioLines from "~icons/lucide/audio-lines";
	import IconChevronDown from "~icons/lucide/chevron-down";
	import IconChevronUp from "~icons/lucide/chevron-up";
	import IconChevronsLeft from "~icons/lucide/chevrons-left";
	import IconChevronsRight from "~icons/lucide/chevrons-right";
	import IconPause from "~icons/lucide/pause";
	import IconPlay from "~icons/lucide/play";
	import IconSkipForward from "~icons/lucide/skip-forward";
	import type { MusicPlayer } from "../../lib/music.svelte";

	let { music }: { music: MusicPlayer } = $props();
</script>

<div class="music-strip">
	<button
		class="music-track"
		title={music.expanded ? "Hide the music panel" : "Show the music panel"}
		onclick={() => (music.expanded = !music.expanded)}
	>
		<IconAudioLines class="music-state" data-state={music.playing ? "playing" : "idle"} />
		<span class="music-title">{music.track}</span>
		<span class="music-playlist">{music.playlist}</span>
		{#if music.expanded}
			<IconChevronUp class="music-disclosure" />
		{:else}
			<IconChevronDown class="music-disclosure" />
		{/if}
	</button>
	<div class="music-actions">
		<IconButton
			size="sm"
			label="Previous playlist"
			onclick={() => void music.previousPlaylist()}
		>
			<IconChevronsLeft />
		</IconButton>
		<IconButton
			size="sm"
			label={music.playing ? "Pause" : "Play"}
			onclick={() => void music.toggle()}
		>
			{#if music.playing}
				<IconPause />
			{:else}
				<IconPlay />
			{/if}
		</IconButton>
		<IconButton
			size="sm"
			label="Next track"
			onclick={() => void music.nextTrack()}
		>
			<IconSkipForward />
		</IconButton>
		<IconButton
			size="sm"
			label="Next playlist"
			onclick={() => void music.nextPlaylist()}
		>
			<IconChevronsRight />
		</IconButton>
	</div>
</div>
