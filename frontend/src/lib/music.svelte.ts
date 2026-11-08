import { createApi, type Api } from "./api";
import type { NowPlaying, Playlist, PlaylistKind } from "./types";

export const POLL_MS = 2_000;

export class MusicPlayer {
	nowPlaying = $state<NowPlaying | null>(null);
	playlists = $state<Playlist[]>([]);
	current = $state(0);
	expanded = $state(false);
	error = $state("");

	#api: Api;
	#timer: ReturnType<typeof setInterval> | null = null;

	constructor(api: Api = createApi()) {
		this.#api = api;
	}

	get playing(): boolean {
		return this.nowPlaying?.status === "playing";
	}

	get track(): string {
		return this.nowPlaying?.track ?? "Nothing is playing";
	}

	get playlist(): string {
		return this.nowPlaying?.playlist ?? "No playlist";
	}

	get volume(): number {
		return this.nowPlaying?.volume ?? 0;
	}

	get shuffle(): boolean {
		return this.nowPlaying?.shuffle ?? false;
	}

	start(): void {
		void this.refresh();
		void this.loadPlaylists();
		this.#timer = setInterval(() => void this.refresh(), POLL_MS);
	}

	stop(): void {
		if (this.#timer) {
			clearInterval(this.#timer);
		}
		this.#timer = null;
	}

	async refresh(): Promise<void> {
		try {
			this.nowPlaying = await this.#api.nowPlaying();
		} catch {
			this.nowPlaying = null;
		}
	}

	async loadPlaylists(): Promise<void> {
		try {
			const page = await this.#api.playlists();
			this.playlists = page.playlists;
			this.current = page.current;
			this.error = "";
		} catch (error) {
			this.error = (error as Error).message;
		}
	}

	async toggle(): Promise<void> {
		await this.control(this.playing ? "pause" : "play");
	}

	async previousPlaylist(): Promise<void> {
		await this.control("prev_playlist");
	}

	async nextPlaylist(): Promise<void> {
		await this.control("next_playlist");
	}

	async nextTrack(): Promise<void> {
		await this.control("next_track");
	}

	async toggleShuffle(): Promise<void> {
		await this.control("toggle_shuffle");
	}

	async control(action: string): Promise<void> {
		try {
			await this.#api.control(action);
			this.error = "";
			await this.refresh();
		} catch (error) {
			this.error = (error as Error).message;
		}
	}

	async setVolume(volume: number): Promise<void> {
		const clamped = Math.min(1, Math.max(0, volume));
		if (this.nowPlaying) {
			this.nowPlaying.volume = clamped;
		}
		try {
			await this.#api.volume(clamped);
			this.error = "";
		} catch (error) {
			this.error = (error as Error).message;
			await this.refresh();
		}
	}

	async add(name: string, kind: PlaylistKind, path: string): Promise<boolean> {
		return this.#playlistAction(() =>
			this.#api.addPlaylist({ name: name.trim(), type: kind, path: path.trim() }),
		);
	}

	async rename(index: number, name: string): Promise<boolean> {
		return this.#playlistAction(() =>
			this.#api.updatePlaylist(index, { name: name.trim() }),
		);
	}

	async changePath(index: number, path: string): Promise<boolean> {
		return this.#playlistAction(() =>
			this.#api.updatePlaylist(index, { path: path.trim() }),
		);
	}

	async remove(index: number): Promise<boolean> {
		return this.#playlistAction(() => this.#api.removePlaylist(index));
	}

	async #playlistAction(action: () => Promise<unknown>): Promise<boolean> {
		try {
			await action();
			this.error = "";
			await this.loadPlaylists();
			await this.refresh();
			return true;
		} catch (error) {
			this.error = (error as Error).message;
			return false;
		}
	}
}
