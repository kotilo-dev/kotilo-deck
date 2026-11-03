export const COLOR_TOKENS = [
	'bg',
	'bg-alt',
	'surface',
	'text',
	'text-muted',
	'border',
	'accent',
	'accent-2',
	'success',
	'warning',
	'danger',
	'info'
] as const;

export type ColorToken = (typeof COLOR_TOKENS)[number];

/** Theme JSON contract shared across Kotilo products (SPEC §5.1). */
export interface Theme {
	id: string;
	name: string;
	variant: 'dark' | 'light';
	pair?: string;
	colors: Record<ColorToken, string>;
	radius: string;
	opacity: { surface: number; bg: number };
	font: { ui: string; mono: string };
}
