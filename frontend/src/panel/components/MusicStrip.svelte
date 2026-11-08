<script lang="ts">
	import Button from "@ku6epxboctuk/ui-kit/components/Button.svelte";
	import type { MusicPlayer } from "../../lib/music.svelte";

	let { music }: { music: MusicPlayer } = $props();
</script>

<div class="music-strip">
	<button
		class="music-track"
		title={music.expanded ? "Hide the music panel" : "Show the music panel"}
		onclick={() => (music.expanded = !music.expanded)}
	>
		<span class="music-dot" data-state={music.playing ? "playing" : "idle"}
		></span>
		<span class="music-title">{music.track}</span>
		<span class="music-playlist">{music.playlist}</span>
	</button>
	<div class="music-actions">
		<Button
			variant="ghost"
			size="sm"
			title="Previous playlist"
			onclick={() => void music.previousPlaylist()}>|&lt;</Button
		>
		<Button variant="ghost" size="sm" onclick={() => void music.toggle()}
			>{music.playing ? "Pause" : "Play"}</Button
		>
		<Button
			variant="ghost"
			size="sm"
			title="Next track"
			onclick={() => void music.nextTrack()}>|&gt;</Button
		>
		<Button
			variant="ghost"
			size="sm"
			title="Next playlist"
			onclick={() => void music.nextPlaylist()}>&gt;&gt;</Button
		>
	</div>
</div>
