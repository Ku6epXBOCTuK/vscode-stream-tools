import { spawn, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import { homedir, tmpdir } from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";
import { type Health, port, probe, send } from "./api";

const START_TIMEOUT_MS = 15000;
const STOP_TIMEOUT_MS = 5000;
const LOCK_STALE_MS = 20000;
const BINARY_NAME =
	process.platform === "win32" ? "stream-tools.exe" : "stream-tools";
const LAUNCHER_LOG = "stream-tools-launcher.log";

const sleep = (ms: number): Promise<void> =>
	new Promise((resolve) => setTimeout(resolve, ms));

function homeExecutable(): string {
	return path.join(homedir(), "bin", "stream-tools", BINARY_NAME);
}

export function resolveExecutable(): string | undefined {
	const configured = vscode.workspace
		.getConfiguration("stream-tools")
		.get<string | null>("binaryPath");
	const candidates = [configured, homeExecutable()];
	for (const candidate of candidates) {
		if (candidate && fs.existsSync(candidate)) {
			return candidate;
		}
	}
	return undefined;
}

function samePath(left: string, right: string): boolean {
	if (!left || !right) {
		return false;
	}
	const normalize = (value: string): string =>
		path.resolve(value).replaceAll("\\", "/").toLowerCase();
	return normalize(left) === normalize(right);
}

function isOurBinary(candidate: string): boolean {
	return path.basename(candidate).toLowerCase() === BINARY_NAME.toLowerCase();
}

function launcherLog(executable: string): string {
	return path.join(path.dirname(executable), LAUNCHER_LOG);
}

function logTail(file: string, lines = 3): string {
	try {
		const content = fs.readFileSync(file, "utf8").trimEnd();
		if (!content) {
			return "";
		}
		return content.split(/\r?\n/).slice(-lines).join("\n");
	} catch {
		return "";
	}
}

interface Lock {
	release: () => void;
}

const noLock: Lock = { release: () => undefined };

function lockFile(): string {
	return path.join(tmpdir(), `stream-tools-${port()}.lock`);
}

function lockHolder(file: string): number {
	try {
		if (Date.now() - fs.statSync(file).mtimeMs > LOCK_STALE_MS) {
			return 0;
		}
		return Number(fs.readFileSync(file, "utf8").split(" ")[0].trim());
	} catch {
		return 0;
	}
}

function lockIsStale(file: string): boolean {
	const pid = lockHolder(file);
	if (!Number.isInteger(pid) || pid <= 0) {
		return true;
	}
	try {
		process.kill(pid, 0);
		return false;
	} catch (error) {
		return (error as NodeJS.ErrnoException).code !== "EPERM";
	}
}

function acquireLock(): Lock | undefined {
	const file = lockFile();
	const token = `${process.pid} ${Date.now()} ${Math.random().toString(36).slice(2)}`;
	for (let attempt = 0; attempt < 2; attempt += 1) {
		try {
			fs.writeFileSync(file, token, { flag: "wx" });
			return {
				release: () => {
					try {
						if (fs.readFileSync(file, "utf8") === token) {
							fs.unlinkSync(file);
						}
					} catch {
						return;
					}
				},
			};
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
				return noLock;
			}
			if (!lockIsStale(file)) {
				return undefined;
			}
			try {
				fs.unlinkSync(file);
			} catch {
				return undefined;
			}
		}
	}
	return undefined;
}

async function waitForBackend(): Promise<boolean> {
	const deadline = Date.now() + START_TIMEOUT_MS;
	while (Date.now() < deadline) {
		await sleep(300);
		if ((await probe()).kind === "ok") {
			return true;
		}
	}
	return false;
}

interface Holder {
	pid: number;
	path: string;
}

function portHolder(): Holder | undefined {
	if (process.platform !== "win32") {
		return undefined;
	}
	const script = [
		`$connection = Get-NetTCPConnection -State Listen -LocalPort ${port()} -ErrorAction SilentlyContinue | Select-Object -First 1`,
		"if ($connection) {",
		"  $process = Get-Process -Id $connection.OwningProcess -ErrorAction SilentlyContinue",
		'  if ($process) { "$($process.Id)|$($process.Path)" }',
		"}",
	].join("\n");
	const result = spawnSync(
		"powershell",
		["-NoProfile", "-NonInteractive", "-Command", script],
		{ encoding: "utf8", windowsHide: true, timeout: 5000 },
	);
	if (result.error || typeof result.stdout !== "string") {
		return undefined;
	}
	const [rawPid, rawPath] = result.stdout.trim().split("|");
	const pid = Number(rawPid);
	if (!Number.isInteger(pid) || pid <= 0) {
		return undefined;
	}
	return { pid, path: rawPath ?? "" };
}

function kill(pid: number): string | undefined {
	try {
		process.kill(pid);
		return undefined;
	} catch (error) {
		return `Cannot stop the process ${pid}: ${(error as Error).message}`;
	}
}

let starting: Promise<string | undefined> | undefined;

export async function start(): Promise<string | undefined> {
	starting ??= launch().finally(() => {
		starting = undefined;
	});
	return starting;
}

async function launch(): Promise<string | undefined> {
	const executable = resolveExecutable();
	if (!executable) {
		return "Backend executable is not found. Run just install or set stream-tools.binaryPath.";
	}

	const lock = acquireLock();
	if (!lock) {
		return (await waitForBackend())
			? undefined
			: `Backend on port ${port()} is being started by another window.`;
	}

	try {
		const log = launcherLog(executable);
		let handle: number | undefined;
		try {
			handle = fs.openSync(log, "a");
		} catch {
			handle = undefined;
		}
		const child = spawn(executable, ["--port", String(port())], {
			detached: true,
			stdio: handle === undefined ? "ignore" : ["ignore", handle, handle],
			windowsHide: true,
		});
		child.unref();
		if (handle !== undefined) {
			fs.closeSync(handle);
		}

		if (await waitForBackend()) {
			return undefined;
		}

		const tail = logTail(log);
		const seconds = Math.round(START_TIMEOUT_MS / 1000);
		return tail
			? `Backend did not answer in ${seconds} seconds. ${executable} said:\n${tail}`
			: `Backend did not answer in ${seconds} seconds. See ${log}.`;
	} finally {
		lock.release();
	}
}

async function stopInstance(_instance: Health): Promise<string | undefined> {
	await send("POST", "/api/shutdown");
	const deadline = Date.now() + STOP_TIMEOUT_MS;
	while (Date.now() < deadline) {
		await sleep(200);
		if ((await probe()).kind !== "ok") {
			return undefined;
		}
	}
	const holder = portHolder();
	return holder ? kill(holder.pid) : undefined;
}

export async function stop(): Promise<string | undefined> {
	const current = await probe();
	if (current.kind !== "ok") {
		return undefined;
	}
	return stopInstance(current.health);
}

export async function restart(): Promise<string | undefined> {
	const stopped = await stop();
	return stopped ?? start();
}

export async function ensure(interactive = true): Promise<string | undefined> {
	const current = await probe();
	const executable = resolveExecutable();

	if (current.kind === "ok") {
		if (!executable || samePath(current.health.exe, executable)) {
			return undefined;
		}
		const stopped = await stopInstance(current.health);
		return stopped ?? start();
	}

	if (current.kind === "unreachable") {
		const holder = portHolder();
		if (holder && !isOurBinary(holder.path)) {
			return `Port ${port()} is taken by another program: ${holder.path} (pid ${holder.pid}).`;
		}
		if (holder) {
			if (!interactive) {
				return `The backend on port ${port()} stopped answering (pid ${holder.pid}).`;
			}
			const answer = await vscode.window.showWarningMessage(
				`Stream Tools: the backend on port ${port()} stopped answering (pid ${holder.pid}). Stop it and start a fresh one?`,
				{ modal: true },
				"Restart",
			);
			if (answer !== "Restart") {
				return undefined;
			}
			const stopped = kill(holder.pid);
			if (stopped) {
				return stopped;
			}
		}
	}

	return start();
}
