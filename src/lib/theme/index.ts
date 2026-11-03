import { SvelteMap } from 'svelte/reactivity';
import { parseTheme, themeToCssVars } from './loader';
import type { Theme } from './types';

export * from './loader';
export * from './types';
export * from './contrast';
export * from './auto';

export const DEFAULT_THEME_ID = 'catppuccin-mocha';

const bundled = import.meta.glob<{ default: unknown }>('./themes/*.json', { eager: true });

const byName = (a: Theme, b: Theme) => a.name.localeCompare(b.name);

/** Themes shipped in `src/lib/theme/themes/` (static). */
export const bundledThemes: ReadonlyMap<string, Theme> = new Map(
	Object.values(bundled)
		.map((m) => parseTheme(m.default))
		.sort(byName)
		.map((theme) => [theme.id, theme])
);

const registry = new SvelteMap<string, Theme>(bundledThemes);
/** Ids currently provided by a user theme file (also when overriding a bundled id). */
const userIds = new SvelteMap<string, true>();
/** Last user-theme load problems: file label → message. */
const issues = new SvelteMap<string, string>();

/**
 * Effective theme registry, sorted by name: bundled themes plus user themes from `~/.kotilo/themes/`.
 * A user theme with the same id as a bundled one replaces it (lets people tweak a bundled palette).
 * Reactive (SvelteMap): readers inside `$derived`/`$effect` update when user themes hot-reload.
 */
export const themes: ReadonlyMap<string, Theme> = registry;

export const isUserTheme = (id: string): boolean => userIds.has(id);
export const userThemeIssues: ReadonlyMap<string, string> = issues;

/** Validates raw user theme JSONs; invalid entries are skipped and reported, never thrown. */
export function parseUserThemes(raw: readonly unknown[]): { themes: Theme[]; errors: { label: string; message: string }[] } {
	const out: Theme[] = [];
	const errors: { label: string; message: string }[] = [];
	const seen = new Set<string>();
	raw.forEach((entry, i) => {
		const r = entry as { id?: unknown; name?: unknown } | null;
		const label = typeof r?.id === 'string' && r.id ? r.id : typeof r?.name === 'string' && r.name ? r.name : `#${i + 1}`;
		try {
			const theme = parseTheme(entry);
			if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(theme.id)) throw new Error(`${theme.id}: id must be lower-case letters, digits and dashes`);
			if (seen.has(theme.id)) throw new Error(`${theme.id}: duplicate id`);
			seen.add(theme.id);
			out.push(theme);
		} catch (e) {
			errors.push({ label, message: e instanceof Error ? e.message : String(e) });
		}
	});
	return { themes: out, errors };
}

/** Replaces the user layer of the registry (bundled themes with the same id are overridden). */
export function setUserThemes(list: readonly Theme[], errors: readonly { label: string; message: string }[] = []): void {
	const merged = new Map(bundledThemes);
	for (const t of list) merged.set(t.id, t);
	registry.clear();
	for (const t of [...merged.values()].sort(byName)) registry.set(t.id, t);
	userIds.clear();
	for (const t of list) userIds.set(t.id, true);
	issues.clear();
	for (const e of errors) issues.set(e.label, e.message);
}

export function getTheme(id: string | null | undefined): Theme {
	return (id ? registry.get(id) : undefined) ?? registry.get(DEFAULT_THEME_ID) ?? bundledThemes.get(DEFAULT_THEME_ID)!;
}

/** Effective theme id (SPEC §4.1): workspace override → global setting → default. */
export function effectiveThemeId(workspaceTheme: string | null | undefined, settingsTheme: string | null | undefined): string {
	return getTheme(workspaceTheme ?? settingsTheme ?? DEFAULT_THEME_ID).id;
}

/**
 * Families whose member names stay distinctive without the family word ("catppuccin mocha" → "mocha").
 * Everything else keeps its full name, because "night", "dark" or "pine dawn" alone are ambiguous.
 */
const DROP_FAMILY = new Set(['catppuccin']);
/** Bundled names that are too long for the chip. */
const SHORT: Record<string, string> = {
	'tokyo-night-day': 'tokyo day',
	'tokyo-night-storm': 'tokyo storm',
	'material-palenight': 'palenight'
};

/** Short label for the top-bar theme chip: "mocha", "dracula", "tokyo night", "tokyo day", "github dark". */
export function themeLabel(theme: Theme): string {
	if (theme.short) return theme.short.toLowerCase();
	if (!isUserTheme(theme.id) && SHORT[theme.id]) return SHORT[theme.id];
	const words = theme.name.toLowerCase().split(/\s+/).filter(Boolean);
	return words.length > 1 && DROP_FAMILY.has(words[0]) ? words.slice(1).join(' ') : words.join(' ');
}

/**
 * Writes the theme's `--k-*` variables onto `root`. Only CSS custom properties change, so a theme
 * switch is a single style recalculation (instant). `surfaceOpacity` = resolved appearance override.
 */
export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement, surfaceOpacity?: number): void {
	for (const [name, value] of Object.entries(themeToCssVars(theme, surfaceOpacity ?? theme.opacity.surface))) {
		root.style.setProperty(name, value);
	}
	root.style.colorScheme = theme.variant;
	root.dataset.theme = theme.id;
}
