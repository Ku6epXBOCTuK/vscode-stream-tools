<script lang="ts">
	import Notice from "@ku6epxboctuk/ui-kit/components/Notice.svelte";
	import { MusicPlayer } from "../../lib/music.svelte";
	import { PanelChat } from "../chat.svelte";
	import Composer from "./Composer.svelte";
	import Feed from "./Feed.svelte";
	import Music from "./Music.svelte";
	import StatusBar from "./StatusBar.svelte";

	const chat = new PanelChat();
	const music = new MusicPlayer();

	$effect(() => {
		void chat.start();
		music.start();
		const flush = () => chat.history.flush();
		window.addEventListener("pagehide", flush);
		return () => {
			window.removeEventListener("pagehide", flush);
			chat.stop();
			music.stop();
		};
	});
</script>

<StatusBar {chat} />
<Music {music} />
<Feed {chat} />
{#if chat.error}
	<Notice tone="danger" variant="box" class="error">
		{chat.error}
	</Notice>
{/if}
<Composer {chat} />
