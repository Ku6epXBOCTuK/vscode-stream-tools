<script lang="ts">
	import IconButton from "@ku6epxboctuk/ui-kit/components/IconButton.svelte";
	import TextInput from "@ku6epxboctuk/ui-kit/components/TextInput.svelte";
	import Notice from "@ku6epxboctuk/ui-kit/components/Notice.svelte";
	import IconFolder from "~icons/lucide/folder";
	import IconFolderOpen from "~icons/lucide/folder-open";
	import IconLink from "~icons/lucide/link";
	import IconListMusic from "~icons/lucide/list-music";
	import IconPencil from "~icons/lucide/pencil";
	import IconPlus from "~icons/lucide/plus";
	import IconRadio from "~icons/lucide/radio";
	import IconShuffle from "~icons/lucide/shuffle";
	import IconTrash2 from "~icons/lucide/trash-2";
	import IconVolume2 from "~icons/lucide/volume-2";
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
			<IconVolume2 class="music-icon" />
			<input
				class="music-volume"
				type="range"
				min="0"
				max="100"
				aria-label="Volume"
				value={Math.round(music.volume * 100)}
				oninput={(event) =>
					void music.setVolume(Number(event.currentTarget.value) / 100)}
			/>
			<span class="music-value">{Math.round(music.volume * 100)}%</span>
			<IconButton
				size="sm"
				label="Shuffle"
				active={music.shuffle}
				onclick={() => void music.toggleShuffle()}
			>
				<IconShuffle />
			</IconButton>
		</div>

		<div class="music-section">
			<IconListMusic class="music-icon" />
			Playlists
		</div>
		<ul class="music-list">
			{#each music.playlists as playlist, index (index)}
				<li class="music-item" class:current={index === music.current}>
					<span
						class="music-kind"
						title={playlist.type === "network_radio" ? "Radio" : "Folder"}
					>
						{#if playlist.type === "network_radio"}
							<IconRadio />
						{:else}
							<IconFolder />
						{/if}
					</span>
					<span class="music-name">{playlist.name}</span>
					<span class="music-path" title={playlist.path}>{playlist.path}</span>
					<IconButton
						size="sm"
						label={`Rename ${playlist.name}`}
						onclick={() => void rename(index, playlist.name)}
					>
						<IconPencil />
					</IconButton>
					<IconButton
						size="sm"
						label={`Change path of ${playlist.name}`}
						onclick={() => void changePath(index, playlist.path)}
					>
						{#if playlist.type === "network_radio"}
							<IconLink />
						{:else}
							<IconFolderOpen />
						{/if}
					</IconButton>
					<IconButton
						size="sm"
						variant="danger"
						label={`Delete ${playlist.name}`}
						onclick={() => void remove(index, playlist.name)}
					>
						<IconTrash2 />
					</IconButton>
				</li>
			{/each}
		</ul>

		<form class="music-add" onsubmit={add}>
			<div class="music-add-row">
				<TextInput bind:value={name} size="sm" placeholder="Playlist name" />
				<IconButton
					size="sm"
					label="Folder"
					active={kind === "local_folder"}
					onclick={() => (kind = "local_folder")}
				>
					<IconFolder />
				</IconButton>
				<IconButton
					size="sm"
					label="Radio"
					active={kind === "network_radio"}
					onclick={() => (kind = "network_radio")}
				>
					<IconRadio />
				</IconButton>
				<IconButton size="sm" variant="secondary" type="submit" label="Add playlist">
					<IconPlus />
				</IconButton>
			</div>
			<TextInput bind:value={path} size="sm" placeholder="C:/Music or https://..." />
		</form>

		{#if music.error}
			<Notice tone="danger" variant="box" class="music-error">
				{music.error}
			</Notice>
		{/if}
	</section>
{/if}
