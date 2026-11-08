import { mount } from "svelte";
import Overlay from "./components/Overlay.svelte";
import "./overlay.css";

const target = document.getElementById("app");

if (!target) {
	throw new Error("missing #app");
}

mount(Overlay, { target });
