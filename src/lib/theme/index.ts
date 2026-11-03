import { parseTheme, themeToCssVars } from './loader';
import type { Theme } from './types';

export * from './loader';
export * from './types';

export const DEFAULT_THEME_ID = 'catppuccin-mocha';

const bundled = import.meta.glob<{ default: unknown }>('./themes/*.json', { eager: true });

export const themes: ReadonlyMap<string, Theme> = new Map(
	Object.values(bundled).map((m) => {
		const theme = parseTheme(m.default);
		return [theme.id, theme];
	})
);

export function getTheme(id: string): Theme {
	return themes.get(id) ?? themes.get(DEFAULT_THEME_ID)!;
}

export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement): void {
	for (const [name, value] of Object.entries(themeToCssVars(theme))) {
		root.style.setProperty(name, value);
	}
	root.style.colorScheme = theme.variant;
	root.dataset.theme = theme.id;
}
