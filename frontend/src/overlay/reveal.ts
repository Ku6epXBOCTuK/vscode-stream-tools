export const REVEAL_MS_PER_PX = 0.6;

export function revealDuration(width: number): number {
	if (!Number.isFinite(width) || width <= 0) {
		return 0;
	}
	return Math.round(width * REVEAL_MS_PER_PX);
}

export function reveal(
	node: HTMLElement,
	duration: (width: number) => number = revealDuration,
): { destroy(): void } {
	const inner = node.querySelector<HTMLElement>(".inner");
	const width = inner ? inner.getBoundingClientRect().width : 0;
	const ms = duration(width);

	node.style.width = "0px";
	let settle = 0;

	const frame = requestAnimationFrame(() => {
		node.style.transition = `width ${ms}ms ease-in-out`;
		node.style.width = `${width}px`;
		settle = window.setTimeout(() => {
			node.classList.add("fixed");
			node.style.transition = "";
			node.style.width = "auto";
		}, ms + 60);
	});

	return {
		destroy() {
			cancelAnimationFrame(frame);
			clearTimeout(settle);
		},
	};
}
