import * as vscode from "vscode";

export interface Health {
	status: string;
	version: string;
	port: number;
	pid: number;
	exe: string;
	uptime_seconds: number;
	music: {
		volume: number;
		shuffle: boolean;
		current_playlist_index: number;
		playlists: number;
	};
	chat: {
		channel: string;
		link: string;
		authenticated: boolean;
		unread: number;
	};
}

const TIMEOUT_MS = 3000;

export function port(): number {
	return vscode.workspace
		.getConfiguration("stream-tools")
		.get<number>("port", 45890);
}

export function baseUrl(): string {
	return `http://127.0.0.1:${port()}`;
}

export async function get<T>(path: string): Promise<T | undefined> {
	try {
		const response = await fetch(`${baseUrl()}${path}`, {
			signal: AbortSignal.timeout(TIMEOUT_MS),
		});
		if (!response.ok) {
			return undefined;
		}
		return (await response.json()) as T;
	} catch {
		return undefined;
	}
}

export async function text(path: string): Promise<string | undefined> {
	try {
		const response = await fetch(`${baseUrl()}${path}`, {
			signal: AbortSignal.timeout(TIMEOUT_MS),
		});
		if (!response.ok) {
			return undefined;
		}
		return await response.text();
	} catch {
		return undefined;
	}
}

export interface Result {
	ok: boolean;
	error?: string;
}

export async function send(
	method: string,
	path: string,
	body?: unknown,
): Promise<Result> {
	try {
		const response = await fetch(`${baseUrl()}${path}`, {
			method,
			headers: body ? { "Content-Type": "application/json" } : undefined,
			body: body ? JSON.stringify(body) : undefined,
			signal: AbortSignal.timeout(5000),
		});
		if (response.ok) {
			return { ok: true };
		}
		const payload = (await response.json().catch(() => undefined)) as
			| { error?: string }
			| undefined;
		return { ok: false, error: payload?.error ?? `HTTP ${response.status}` };
	} catch (error) {
		return { ok: false, error: (error as Error).message };
	}
}

export type Probe =
	| { kind: "ok"; health: Health }
	| { kind: "unreachable" }
	| { kind: "offline" };

export async function probe(): Promise<Probe> {
	try {
		const response = await fetch(`${baseUrl()}/api/health`, {
			signal: AbortSignal.timeout(TIMEOUT_MS),
		});
		if (!response.ok) {
			return { kind: "unreachable" };
		}
		return { kind: "ok", health: (await response.json()) as Health };
	} catch (error) {
		const name = (error as { name?: string }).name;
		return { kind: name === "TimeoutError" ? "unreachable" : "offline" };
	}
}

export async function health(): Promise<Health | undefined> {
	const result = await probe();
	return result.kind === "ok" ? result.health : undefined;
}
