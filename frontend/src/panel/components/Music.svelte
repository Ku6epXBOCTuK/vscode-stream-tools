<script lang="ts">
	import Button from "@ku6epxboctuk/ui-kit/components/Button.svelte";
	import TextInput from "@ku6epxboctuk/ui-kit/components/TextInput.svelte";
	import Notice from "@ku6epxboctuk/ui-kit/components/Notice.svelte";
	import type { MusicPlayer } from "../../lib/music.svelte";
	import type { PlaylistKind } from "../../lib/types";
	import MusicStrip from "./MusicStrip.svelte";

	let { music }: { music: MusicPlayer } = $props();

	let name = $state("");
	let kind = $state<PlaylistKind>("local_folder");
	let path = $state("");

	async function add(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (!name.trim() || !path.trim()) {
			return;
		}
		if (await music.add(name, kind, path)) {
			name = "";
			path = "";
		}
	}

	async function rename(index: number, current: string): Promise<void> {
		const next = window.prompt("Playlist name", current);
		if (next && next.trim() !== current) {
			await music.rename(index, next);
		}
	}

	async function changePath(index: number, current: string): Promise<void> {
		const next = window.prompt("Folder or stream URL", current);
		if (next && next.trim() !== current) {
			await music.changePath(index, next);
		}
	}

	async function remove(index: number, name: string): Promise<void> {
		if (window.confirm(`Delete the playlist "${name}"?`)) {
			await music.remove(index);
		}
	}
</script>

<MusicStrip {music} />

{#if music.expanded}
	<section class="music-panel">
		<div class="music-row">
			<label class="music-label" for="music-volume">Volume</label>
			<input
				id="music-volume"
				class="music-volume"
				type="range"
				min="0"
				max="100"
				value={Math.round(music.volume * 100)}
				oninput={(event) =>
					void music.setVolume(Number(event.currentTarget.value) / 100)}
			/>
			<span class="music-value">{Math.round(music.volume * 100)}%</span>
			<Button
				variant={music.shuffle ? "secondary" : "ghost"}
				size="sm"
				onclick={() => void music.toggleShuffle()}
				>Shuffle {music.shuffle ? "on" : "off"}</Button
			>
		</div>

		<div class="music-section">Playlists</div>
		<ul class="music-list">
			{#each music.playlists as playlist, index (index)}
				<li class="music-item" class:current={index === music.current}>
					<span class="music-name">{playlist.name}</span>
					<span class="music-kind">{playlist.type === "network_radio" ? "radio" : "folder"}</span>
					<span class="music-path" title={playlist.path}>{playlist.path}</span>
					<Button
						variant="ghost"
						size="sm"
						onclick={() => void rename(index, playlist.name)}>Rename</Button
					>
					<Button
						variant="ghost"
						size="sm"
						onclick={() => void changePath(index, playlist.path)}>Path</Button
					>
					<Button
						variant="ghost"
						size="sm"
						title="Delete playlist"
						onclick={() => void remove(index, playlist.name)}>Delete</Button
					>
				</li>
			{/each}
		</ul>

		<form class="music-add" onsubmit={add}>
			<TextInput bind:value={name} size="sm" placeholder="Name" />
			<select class="music-select" bind:value={kind}>
				<option value="local_folder">Folder</option>
				<option value="network_radio">Radio</option>
			</select>
			<TextInput
				bind:value={path}
				size="sm"
				placeholder="C:/Music or https://..."
			/>
			<Button variant="secondary" size="sm" type="submit">Add</Button>
		</form>

		{#if music.error}
			<Notice tone="danger" variant="box" class="music-error">
				{music.error}
			</Notice>
		{/if}
	</section>
{/if}
