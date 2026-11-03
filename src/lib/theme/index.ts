import { parseTheme, themeToCssVars } from './loader';
import type { Theme } from './types';

export * from './loader';
export * from './types';
export * from './contrast';

export const DEFAULT_THEME_ID = 'catppuccin-mocha';

const bundled = import.meta.glob<{ default: unknown }>('./themes/*.json', { eager: true });

export const themes: ReadonlyMap<string, Theme> = new Map(
	Object.values(bundled)
		.map((m) => parseTheme(m.default))
		.sort((a, b) => a.name.localeCompare(b.name))
		.map((theme) => [theme.id, theme])
);

export function getTheme(id: string | null | undefined): Theme {
	return (id ? themes.get(id) : undefined) ?? themes.get(DEFAULT_THEME_ID)!;
}

/** Effective theme id (SPEC §4.1): workspace override → global setting → default. */
export function effectiveThemeId(workspaceTheme: string | null | undefined, settingsTheme: string | null | undefined): string {
	return getTheme(workspaceTheme ?? settingsTheme ?? DEFAULT_THEME_ID).id;
}

/** Short label for the top-bar theme chip: "mocha", "latte", "dracula", "tokyo night". */
export function themeLabel(theme: Theme): string {
	const words = theme.name.toLowerCase().split(/\s+/);
	return theme.pair && words.length > 1 ? words.slice(1).join(' ') : words.join(' ');
}

/**
 * Writes the theme's `--k-*` variables onto `root`. Only CSS custom properties change, so a theme
 * switch is a single style recalculation (instant). `surfaceOpacity` = workspace appearance override.
 */
export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement, surfaceOpacity?: number): void {
	for (const [name, value] of Object.entries(themeToCssVars(theme, surfaceOpacity ?? theme.opacity.surface))) {
		root.style.setProperty(name, value);
	}
	root.style.colorScheme = theme.variant;
	root.dataset.theme = theme.id;
}
