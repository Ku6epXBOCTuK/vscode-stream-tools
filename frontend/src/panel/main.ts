import { mount } from "svelte";
import "@ku6epxboctuk/ui-kit/tokens.css";
import Panel from "./components/Panel.svelte";
import "./vscode.css";
import "./panel.css";

const target = document.getElementById("app");

if (!target) {
	throw new Error("missing #app");
}

document.body.classList.add("panel");

function syncUiMode(): void {
	const dark =
		document.body.classList.contains("vscode-dark") ||
		document.body.classList.contains("vscode-high-contrast");
	document.documentElement.dataset.uiMode = dark ? "dark" : "light";
}

syncUiMode();
new MutationObserver(syncUiMode).observe(document.body, {
	attributes: true,
	attributeFilter: ["class"],
});

mount(Panel, { target });
