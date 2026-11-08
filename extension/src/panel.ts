import { randomBytes } from "node:crypto";
import * as vscode from "vscode";
import { baseUrl, health, text } from "./api";

export class ChatView implements vscode.WebviewViewProvider {
	static readonly viewId = "stream-tools.chat";

	private view: vscode.WebviewView | undefined;
	private loaded = false;

	constructor(private readonly onVisibility: (visible: boolean) => void) {}

	resolveWebviewView(view: vscode.WebviewView): void {
		this.view = view;
		view.onDidChangeVisibility(() => this.onVisibility(view.visible));
		view.onDidDispose(() => {
			this.view = undefined;
			this.loaded = false;
		});
		void this.render();
	}

	async render(): Promise<void> {
		if (!this.view) {
			return;
		}
		const page = await this.page();
		if (!this.view) {
			return;
		}
		if (page) {
			this.view.webview.options = {
				enableScripts: true,
				enableCommandUris: true,
			};
			this.view.webview.html = page;
		}
		this.onVisibility(this.view.visible);
	}

	async reload(): Promise<void> {
		this.loaded = false;
		await this.render();
	}

	private async page(): Promise<string | undefined> {
		if (this.loaded) {
			return undefined;
		}
		if ((await health()) !== undefined) {
			const panel = await inlinePanel();
			if (panel) {
				this.loaded = true;
				return panel;
			}
		}
		return offline();
	}

	async reveal(): Promise<void> {
		await vscode.commands.executeCommand(`${ChatView.viewId}.focus`);
	}
}

async function inlinePanel(): Promise<string | undefined> {
	const origin = baseUrl();
	const [html, css, script] = await Promise.all([
		text("/panel"),
		text("/assets/app/panel.css"),
		text("/assets/app/panel.js"),
	]);
	if (!html || !css || !script) {
		return undefined;
	}

	const nonce = randomBytes(16).toString("base64");
	const policy = [
		"default-src 'none'",
		"style-src 'unsafe-inline'",
		`script-src 'nonce-${nonce}'`,
		`connect-src ${origin}`,
		`img-src ${origin} https://static-cdn.jtvnw.net data:`,
		`font-src ${origin}`,
	].join("; ");

	return html
		.replace(
			/<head>/i,
			`<head><base href="${origin}/"><meta http-equiv="Content-Security-Policy" content="${policy}" />`,
		)
		.replace(/<link[^>]+fonts\.css[^>]*>/i, "")
		.replace(/<link[^>]+panel\.css[^>]*>/i, () => `<style>${css}</style>`)
		.replace(
			/<script[^>]+panel\.js[^>]*><\/script>/i,
			() => `<script type="module" nonce="${nonce}">${script}</script>`,
		);
}

function offline(): string {
	return `<!doctype html>
<html lang="en">
	<head>
		<meta charset="utf-8" />
		<meta
			http-equiv="Content-Security-Policy"
			content="default-src 'none'; style-src 'unsafe-inline';"
		/>
		<style>
			body {
				font-family: var(--vscode-font-family);
				color: var(--vscode-foreground);
				padding: 16px;
				font-size: 13px;
			}
			p {
				margin: 0 0 12px;
			}
			a {
				display: block;
				margin-bottom: 8px;
				color: var(--vscode-textLink-foreground);
				text-decoration: none;
			}
			a:hover {
				text-decoration: underline;
			}
		</style>
	</head>
	<body>
		<p>The stream-tools backend is not running.</p>
		<a href="command:stream-tools.startServer">Start the backend</a>
		<a href="command:stream-tools.signIn">Sign in to Twitch</a>
		<a href="command:stream-tools.openPanel">Open the panel in a browser</a>
	</body>
</html>`;
}
