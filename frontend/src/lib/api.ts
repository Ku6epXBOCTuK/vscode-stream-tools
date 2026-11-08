import type {
	Badges,
	ChatMessage,
	MessagesPage,
	NowPlaying,
	Playlist,
	PlaylistsPage,
	State,
} from "./types";

export type Fetcher = (path: string, init?: RequestInit) => Promise<Response>;

const JSON_HEADERS = { "Content-Type": "application/json" };

async function request<T>(
	fetcher: Fetcher,
	path: string,
	init?: RequestInit,
): Promise<T> {
	const response = await fetcher(path, init);
	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		throw new Error(payload?.error || `HTTP ${response.status}`);
	}
	return payload as T;
}

export type Api = ReturnType<typeof createApi>;

export function createApi(
	fetcher: Fetcher = (path, init) => fetch(path, init),
) {
	return {
		state: () => request<State>(fetcher, "/chat/state"),
		badges: () => request<Badges>(fetcher, "/chat/badges"),
		messages: (after: number, limit = 200) =>
			request<MessagesPage>(
				fetcher,
				`/chat/messages?after=${after}&limit=${limit}`,
			),
		send: (text: string, replyTo?: string) =>
			request<ChatMessage>(fetcher, "/chat/messages", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify({ text, reply_to: replyTo || undefined }),
			}),
		remove: (id: string) =>
			request<unknown>(fetcher, `/chat/messages/${encodeURIComponent(id)}`, {
				method: "DELETE",
			}),
		timeout: (userId: string, seconds: number, reason: string) =>
			request<unknown>(fetcher, "/chat/moderation/timeout", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify({ user_id: userId, duration: seconds, reason }),
			}),
		ban: (userId: string, reason: string) =>
			request<unknown>(fetcher, "/chat/moderation/ban", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify({ user_id: userId, reason }),
			}),
		nowPlaying: () => request<NowPlaying>(fetcher, "/music/now_playing"),
		playlists: () => request<PlaylistsPage>(fetcher, "/music/playlists"),
		control: (action: string) =>
			request<unknown>(fetcher, "/music/control", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify({ action }),
			}),
		volume: (volume: number) =>
			request<unknown>(fetcher, "/music/volume", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify({ volume }),
			}),
		addPlaylist: (playlist: Playlist) =>
			request<unknown>(fetcher, "/music/playlists", {
				method: "POST",
				headers: JSON_HEADERS,
				body: JSON.stringify(playlist),
			}),
		updatePlaylist: (index: number, patch: Partial<Playlist>) =>
			request<unknown>(fetcher, `/music/playlists/${index}`, {
				method: "PATCH",
				headers: JSON_HEADERS,
				body: JSON.stringify(patch),
			}),
		removePlaylist: (index: number) =>
			request<unknown>(fetcher, `/music/playlists/${index}`, {
				method: "DELETE",
			}),
	};
}
