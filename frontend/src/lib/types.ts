export type LinkState = "connected" | "connecting" | "offline";

export type Badge = {
	name: string;
	version: string;
};

export type User = {
	id: string;
	login: string;
	name: string;
	color: string | null;
	badges: Badge[];
	moderator: boolean;
	subscriber: boolean;
	vip: boolean;
	broadcaster: boolean;
};

export type Reply = {
	id: string;
	user: string;
	text: string;
};

export type Fragment =
	| { type: "text"; text: string }
	| { type: "emote"; text: string; id: string }
	| {
			type: "cheermote";
			text: string;
			prefix: string;
			bits: number;
			tier: number;
	  }
	| { type: "mention"; text: string; login: string; name: string };

export type ChatEvent = {
	kind: string;
	summary: string;
	amount?: number;
};

export type ChatMessage = {
	seq: number;
	id: string;
	channel: string;
	ts: string;
	kind: string;
	user: User;
	text: string;
	fragments: Fragment[];
	reply?: Reply;
	action: boolean;
	highlighted: boolean;
	moderatable: boolean;
	event?: ChatEvent;
};

export type MessagesPage = {
	items: ChatMessage[];
	last_seq: number;
};

export type Badges = {
	badges: Record<string, string>;
};

export type Widget = {
	max_messages: number;
	fade_seconds: number;
	font_size: number;
	show_badges: boolean;
};

export type State = {
	channel: string;
	port: number;
	authenticated: boolean;
	login: string;
	widget: Widget;
	eventsub: LinkState;
	messages: number;
};

export type PlaylistKind = "local_folder" | "network_radio";

export type Playlist = {
	name: string;
	type: PlaylistKind;
	path: string;
};

export type NowPlaying = {
	status: "playing" | "paused" | "stopped";
	track: string | null;
	volume: number;
	playlist_index: number;
	playlist: string | null;
	playlist_type: PlaylistKind | null;
	shuffle: boolean;
};

export type PlaylistsPage = {
	current: number;
	playlists: Playlist[];
};
