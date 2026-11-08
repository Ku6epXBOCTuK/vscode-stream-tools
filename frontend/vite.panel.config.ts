import { defineConfig } from "vite";
import { entryConfig } from "./vite.base.ts";

export default defineConfig(entryConfig("panel.html", "panel", true));
