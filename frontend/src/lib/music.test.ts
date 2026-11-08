import { describe, expect, it, vi } from "vitest";
import { createApi, type Fetcher } from "./api";
import { MusicPlayer } from "./music.svelte";

function fetcher(calls: { path: string; init?: RequestInit }[]): Fetcher {
	const responses: Record<string, unknown> = {
		"/music/now_playing": {
			status: "playing",
			track: "song.flac",
			volume: 0.5,
			playlist_index: 0,
			playlist: "Local",
			playlist_type: "local_folder",
			shuffle: false,
		},
		"/music/playlists": {
			current: 0,
			playlists: [{ name: "Local", type: "local_folder", path: "C:/Music" }],
		},
	};
	return vi.fn(async (path: string, init?: RequestInit) => {
		calls.push({ path, init });
		return {
			ok: true,
			status: 200,
			json: async () => responses[path] ?? {},
		} as Response;
	});
}

describe("music player", () => {
	it("loads the now playing track and the playlists", async () => {
		const calls: { path: string; init?: RequestInit }[] = [];
		const music = new MusicPlayer(createApi(fetcher(calls)));

		await music.refresh();
		await music.loadPlaylists();

		expect(music.track).toBe("song.flac");
		expect(music.playlist).toBe("Local");
		expect(music.playlists).toHaveLength(1);
		expect(calls.map((call) => call.path)).toEqual([
			"/music/now_playing",
			"/music/playlists",
		]);
	});

	it("sends the control action and clamps the volume", async () => {
		const calls: { path: string; init?: RequestInit }[] = [];
		const music = new MusicPlayer(createApi(fetcher(calls)));

		await music.control("next_track");
		await music.setVolume(4);

		expect(calls[0].path).toBe("/music/control");
		expect(calls[0].init?.body).toBe('{"action":"next_track"}');
		const volume = calls.find((call) => call.path === "/music/volume");
		expect(volume?.init?.body).toBe('{"volume":1}');
	});
});
