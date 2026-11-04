/** Theme payload for plugins: the computed `--k-*` values, read at the plugin's own card. */
import { COLOR_TOKENS } from '#lib/theme/index.ts';
import type { PluginTheme } from './bridge';

/** Always sent (theme loader output, DESIGN §2). Inline `--k-*` on the root are added on top. */
export const THEME_TOKENS: readonly string[] = [...COLOR_TOKENS, 'surface-a', 'on-accent', 'radius', 'font-ui', 'font-mono'];
/** Runtime deck state, not theme: the deck already applies them to the whole content layer. */
const RUNTIME = new Set(['dim', 'backdrop']);

export interface StyleSource {
	/** Custom property names declared inline on the deck root (where applyTheme writes them). */
	names(): Iterable<string>;
	/** Computed value at the plugin's card, so per-instance overrides (surface opacity) are included. */
	value(name: string): string;
	colorScheme(): string;
}

export function readTheme(src: StyleSource): PluginTheme {
	const keys = new Set(THEME_TOKENS);
	for (const n of src.names()) {
		if (!n.startsWith('--k-')) continue;
		const key = n.slice(4);
		if (!RUNTIME.has(key)) keys.add(key);
	}
	const tokens: Record<string, string> = {};
	for (const key of keys) {
		const v = src.value(`--k-${key}`).trim();
		if (v) tokens[key] = v;
	}
	return { variant: /\blight\b/.test(src.colorScheme()) && !/\bdark\b/.test(src.colorScheme()) ? 'light' : 'dark', tokens };
}

/** DOM source: names from `root.style`, values from `getComputedStyle(el)`. */
export function domStyleSource(root: HTMLElement, el: Element): StyleSource {
	const computed = getComputedStyle(el);
	return {
		names: () => Array.from(root.style).filter((n) => n.startsWith('--k-')),
		value: (n) => computed.getPropertyValue(n),
		colorScheme: () => root.style.colorScheme || getComputedStyle(root).colorScheme || 'dark'
	};
}
