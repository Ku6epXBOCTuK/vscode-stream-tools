import * as vscode from "vscode";
import { type Health, baseUrl, get, probe, send } from "./api";
import { ensure, restart, stop } from "./backend";
import { ChatView } from "./panel";
import {
	MAX_ALBUM_LENGTH,
	MAX_TRACK_LENGTH,
	type NowPlaying,
	albumOf,
	fullTitle,
	trackLabel,
	truncate,
} from "./track";
import { UnreadBadge } from "./unread";

const POLL_MS = 3000;
const VOLUME_STEP_PERCENT = 10;
const AUTO_START_AFTER_FAILURES = 2;
const AUTO_START_ATTEMPTS = 5;
const AUTO_START_FIRST_GAP_MS = 5000;
const AUTO_START_MAX_GAP_MS = 60000;

let status: vscode.StatusBarItem | undefined;
let timer: ReturnType<typeof setInterval> | undefined;
let unread = 0;
let failures = 0;
let attempts = 0;
let nextAttemptAt = 0;
let autoStarting = false;
let userStopped = false;
let lastHealth: Health | undefined;
let nowPlaying: NowPlaying | undefined;

function autoStart(): boolean {
	return vscode.workspace
		.getConfiguration("stream-tools")
		.get<boolean>("autoStart", true);
}

function musicText(state: NowPlaying): string {
	const icon = state.status === "playing" ? "$(music)" : "$(debug-pause)";
	const track = truncate(trackLabel(state), MAX_TRACK_LENGTH);
	const album = albumOf(state);
	return album
		? `${icon} ${truncate(album, MAX_ALBUM_LENGTH)} - ${track}`
		: `${icon} ${track}`;
}

function tooltip(health: Health): string {
	const lines = [
		`Stream Tools ${health.version}`,
		`Twitch: ${health.chat.link}${health.chat.channel ? ` (#${health.chat.channel})` : ""}`,
	];
	if (nowPlaying) {
		lines.push(`${nowPlaying.status}: ${fullTitle(nowPlaying)}`);
	}
	const volume = nowPlaying?.volume ?? health.music.volume;
	lines.push(`Volume: ${Math.round(volume * 100)}%`);
	lines.push(unread > 0 ? `${unread} unread` : "No unread messages");
	return lines.join("\n");
}

export function activate(context: vscode.ExtensionContext): void {
	let visible = false;
	let focused = vscode.window.state.focused;

	status = vscode.window.createStatusBarItem(
		vscode.StatusBarAlignment.Right,
		60,
	);
	status.command = "stream-tools.openChat";
	status.tooltip = "Stream Tools";
	status.show();

	const badge = new UnreadBadge();

	const chat = new ChatView((isVisible) => {
		visible = isVisible;
		void poll();
	});

	const poll = async (): Promise<void> => {
		if (!status) {
			return;
		}
		const result = await probe();
		if (result.kind !== "ok") {
			failures += 1;
			unread = 0;
			badge.set(0);
			lastHealth = undefined;
			nowPlaying = undefined;
			status.text = "$(debug-disconnect)";
			status.tooltip =
				result.kind === "unreachable"
					? `Backend on port ${vscode.workspace
							.getConfiguration("stream-tools")
							.get<number>("port", 45890)} is not answering`
					: "Stream Tools backend is not running";
			void recover();
			return;
		}

		failures = 0;
		attempts = 0;
		nextAttemptAt = 0;

		lastHealth = result.health;
		nowPlaying = await get<NowPlaying>("/music/now_playing");
		const info = result.health;
		if (visible && focused) {
			if (info.chat.unread > 0) {
				await send("POST", "/chat/read");
			}
			unread = 0;
		} else {
			unread = info.chat.unread;
		}
		badge.set(unread);
		status.text = nowPlaying
			? musicText(nowPlaying)
			: "$(music) Nothing is playing";
		if (unread > 0) {
			status.text += `  $(comment-discussion) ${unread}`;
		}
		status.tooltip = tooltip(info);
	};

	const recover = async (): Promise<void> => {
		if (autoStarting || userStopped || !autoStart()) {
			return;
		}
		if (failures < AUTO_START_AFTER_FAILURES) {
			return;
		}
		if (attempts >= AUTO_START_ATTEMPTS || Date.now() < nextAttemptAt) {
			return;
		}
		autoStarting = true;
		attempts += 1;
		nextAttemptAt =
			Date.now() +
			Math.min(
				AUTO_START_MAX_GAP_MS,
				AUTO_START_FIRST_GAP_MS * 2 ** (attempts - 1),
			);
		try {
			const error = await ensure(attempts === 1);
			if (error) {
				void vscode.window.showErrorMessage(`Stream Tools: ${error}`);
			}
		} catch (error) {
			void vscode.window.showErrorMessage(
				`Stream Tools: ${(error as Error).message}`,
			);
		} finally {
			autoStarting = false;
			failures = 0;
		}
	};

	const refresh = async (): Promise<void> => {
		await poll();
		await chat.reload();
	};

	const control = async (action: string): Promise<void> => {
		const result = await send("POST", "/music/control", { action });
		if (!result.ok) {
			void vscode.window.showWarningMessage(
				`Stream Tools: ${result.error ?? "music is not reachable"}`,
			);
			return;
		}
		setTimeout(() => void poll(), 250);
	};

	const setVolume = async (volume: number): Promise<void> => {
		const clamped = Math.min(1, Math.max(0, volume));
		const result = await send("POST", "/music/volume", { volume: clamped });
		if (!result.ok) {
			void vscode.window.showWarningMessage(
				`Stream Tools: ${result.error ?? "music is not reachable"}`,
			);
			return;
		}
		setTimeout(() => void poll(), 250);
	};

	const pickVolume = async (): Promise<void> => {
		const current = nowPlaying ? Math.round(nowPlaying.volume * 100) : -1;
		const choices: vscode.QuickPickItem[] = [];
		for (let percent = 0; percent <= 100; percent += VOLUME_STEP_PERCENT) {
			choices.push({
				label: `${percent}%`,
				description: percent === current ? "current" : undefined,
			});
		}
		const picked = await vscode.window.showQuickPick(choices, {
			placeHolder: "Volume (0-100)",
		});
		if (picked) {
			await setVolume(Number.parseInt(picked.label, 10) / 100);
		}
	};

	const withServer = async (
		run: () => Promise<string | undefined>,
	): Promise<void> => {
		const error = await run();
		if (error) {
			void vscode.window.showErrorMessage(`Stream Tools: ${error}`);
		}
		await refresh();
	};

	const playPause = async (): Promise<void> => {
		if (!nowPlaying) {
			userStopped = false;
			await withServer(() => ensure());
			return;
		}
		await control(nowPlaying.status === "playing" ? "pause" : "play");
	};

	context.subscriptions.push(
		status,
		badge,
		vscode.window.onDidChangeWindowState((state) => {
			focused = state.focused;
			void poll();
		}),
		vscode.window.registerWebviewViewProvider(ChatView.viewId, chat, {
			webviewOptions: { retainContextWhenHidden: true },
		}),
		vscode.commands.registerCommand("stream-tools.openChat", async () => {
			await chat.reveal();
		}),
		vscode.commands.registerCommand("stream-tools.refresh", async () => {
			userStopped = false;
			await refresh();
		}),
		vscode.commands.registerCommand("stream-tools.startServer", async () => {
			userStopped = false;
			await withServer(() => ensure());
		}),
		vscode.commands.registerCommand("stream-tools.stopServer", async () => {
			userStopped = true;
			await withServer(stop);
		}),
		vscode.commands.registerCommand("stream-tools.restartServer", async () => {
			userStopped = false;
			await withServer(restart);
		}),
		vscode.commands.registerCommand(
			"stream-tools.reconnectChat",
			async () => {
				const result = await send("POST", "/chat/eventsub/reconnect");
				if (!result.ok) {
					void vscode.window.showErrorMessage(
						`Stream Tools: ${result.error}`,
					);
				}
				await poll();
			},
		),
		vscode.commands.registerCommand("stream-tools.signIn", async () => {
			const login = await get<{ url: string }>("/chat/auth/url");
			if (!login) {
				void vscode.window.showWarningMessage(
					"Stream Tools: put client_id and client_secret of your Twitch application into config.json next to the backend.",
				);
				return;
			}
			await vscode.env.openExternal(vscode.Uri.parse(login.url));
		}),
		vscode.commands.registerCommand("stream-tools.setChannel", async () => {
			const current = lastHealth?.chat.channel ?? "";
			const channel = await vscode.window.showInputBox({
				prompt: "Twitch channel login",
				value: current,
				validateInput: (value) =>
					/^#?[a-zA-Z0-9_]{1,25}$/.test(value.trim())
						? undefined
						: "1-25 letters, digits or underscores",
			});
			if (!channel) {
				return;
			}
			const result = await send("POST", "/chat/channel", {
				channel: channel.trim(),
			});
			if (!result.ok) {
				void vscode.window.showErrorMessage(`Stream Tools: ${result.error}`);
				return;
			}
			const action = await vscode.window.showInformationMessage(
				`Channel set to ${channel.trim()}. Restart the backend to connect.`,
				"Restart",
			);
			if (action === "Restart") {
				userStopped = false;
				await withServer(restart);
			}
		}),
		vscode.commands.registerCommand("stream-tools.copyOverlayUrl", async () => {
			const url = `${baseUrl()}/overlay`;
			await vscode.env.clipboard.writeText(url);
			void vscode.window.showInformationMessage(
				`OBS browser source URL copied: ${url}`,
			);
		}),
		vscode.commands.registerCommand("stream-tools.openPanel", async () => {
			await vscode.env.openExternal(vscode.Uri.parse(`${baseUrl()}/panel`));
		}),
		vscode.commands.registerCommand("stream-tools.playPause", playPause),
		vscode.commands.registerCommand("stream-tools.nextTrack", () =>
			control("next_track"),
		),
		vscode.commands.registerCommand("stream-tools.nextPlaylist", () =>
			control("next_playlist"),
		),
		vscode.commands.registerCommand("stream-tools.previousPlaylist", () =>
			control("prev_playlist"),
		),
		vscode.commands.registerCommand("stream-tools.toggleShuffle", () =>
			control("toggle_shuffle"),
		),
		vscode.commands.registerCommand("stream-tools.setVolume", () =>
			pickVolume(),
		),
		vscode.workspace.onDidChangeConfiguration((event) => {
			if (event.affectsConfiguration("stream-tools")) {
				void refresh();
			}
		}),
	);

	void refresh();
	timer = setInterval(() => void poll(), POLL_MS);
}

export function deactivate(): void {
	if (timer) {
		clearInterval(timer);
		timer = undefined;
	}
	status?.dispose();
	status = undefined;
}
