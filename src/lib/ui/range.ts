/**
 * Shared `.k-range` fill (DESIGN §5.6): the native `<input type="range">` stays for keyboard and screen
 * readers; WebKit has no progress pseudo-element, so the filled part is a gradient stop at `--p`.
 */

/** Filled share of a range input, as a CSS percentage string (0–100 %). */
export function rangePercent(value: number, min: number, max: number): string {
	if (!Number.isFinite(value) || !Number.isFinite(min) || !Number.isFinite(max) || max <= min) return '0%';
	const pct = ((Math.min(max, Math.max(min, value)) - min) / (max - min)) * 100;
	return `${Math.round(pct * 100) / 100}%`;
}

/**
 * Svelte action: keeps `--p` on a range input in step with its value. Pass the bound value as the
 * parameter so programmatic changes (draft reset, remote settings) repaint too.
 */
export function rangeFill(node: HTMLInputElement, _value?: unknown) {
	const paint = () => node.style.setProperty('--p', rangePercent(Number(node.value), Number(node.min || 0), Number(node.max || 100)));
	paint();
	node.addEventListener('input', paint);
	return {
		update: () => queueMicrotask(paint),
		destroy: () => node.removeEventListener('input', paint)
	};
}
