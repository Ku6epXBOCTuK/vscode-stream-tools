import { resolve } from "node:path";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import type { UserConfig } from "vite";

const root = import.meta.dirname;
const kitComponents = resolve(
	root,
	"node_modules/@ku6epxboctuk/ui-kit/src/components",
);

export function entryConfig(
	html: string,
	name: string,
	emptyOutDir: boolean,
): UserConfig {
	return {
		base: "/assets/app/",
		resolve: {
			tsconfigPaths: false,
			alias: [
				{
					find: /^@ku6epxboctuk\/ui-kit\/components\/(.+)$/,
					replacement: `${kitComponents}/$1`,
				},
			],
		},
		plugins: [svelte()],
		build: {
			outDir: "../crates/app/assets/app",
			emptyOutDir,
			cssCodeSplit: false,
			assetsInlineLimit: 0,
			rollupOptions: {
				input: resolve(root, html),
				output: {
					format: "iife",
					entryFileNames: `${name}.js`,
					assetFileNames: `${name}.[ext]`,
				},
			},
		},
	};
}
