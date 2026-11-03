/**
 * Inline stroke icons (Lucide-style geometry, 24×24 viewBox, 2 px stroke, round caps; DESIGN §8).
 * Widget manifests reference these by `icon` key. Each entry is a list of SVG path `d` strings.
 */
export const ICONS = {
	clock: ['M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z', 'M12 7v5l3 2'],
	calendar: ['M5 5h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z', 'M16 3v4M8 3v4M3 11h18'],
	cloud: ['M17.5 19H8a5 5 0 1 1 .9-9.9A6 6 0 0 1 20 11.5a3.75 3.75 0 0 1-2.5 7.5Z'],
	sun: ['M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z', 'M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4'],
	cpu: ['M3 17l5-6 4 3 5-8 4 5'],
	activity: ['M3 17l5-6 4 3 5-8 4 5'],
	timer: ['M12 5a8 8 0 1 0 0 16 8 8 0 0 0 0-16Z', 'M12 9v4l2 2M9 2h6'],
	check: ['M20 6 9 17l-5-5'],
	todo: ['M9 11l3 3L22 4', 'M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11'],
	music: ['M9 18V5l12-2v13', 'M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z', 'M18 19a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z'],
	volume: ['M11 5 6 9H2v6h4l5 4V5Z', 'M15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14'],
	agents: ['M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z', 'M8 20h8M12 16v4', 'M12 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z'],
	puzzle: [
		'M19.4 12.6a2 2 0 1 0 0-3.2V6a1 1 0 0 0-1-1h-3.4a2 2 0 1 0-3.2 0H8.4a1 1 0 0 0-1 1v3.4a2 2 0 1 0 0 3.2V16a1 1 0 0 0 1 1h3.4a2 2 0 1 0 3.2 0h3.4a1 1 0 0 0 1-1Z'
	],
	edit: ['M12 20h9', 'M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z'],
	gear: [
		'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
		'M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z'
	],
	plus: ['M12 5v14M5 12h14'],
	x: ['M18 6 6 18M6 6l12 12'],
	trash: ['M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6'],
	resize: ['M20 4 4 20M20 12l-8 8'],
	chevronDown: ['m6 9 6 6 6-6'],
	alert: ['M12 3 2 20h20Z', 'M12 10v4M12 17h.01'],
	refresh: ['M21 12a9 9 0 1 1-2.6-6.4L21 8', 'M21 3v5h-5'],
	copy: ['M9 9h11v11H9Z', 'M5 15H4V4h11v1'],
	upload: ['M12 15V3M7 8l5-5 5 5', 'M5 21h14'],
	download: ['M12 3v12M7 10l5 5 5-5', 'M5 21h14'],
	palette: ['M12 3a9 9 0 1 0 0 18c1 0 1.5-.7 1.5-1.5 0-.4-.2-.8-.4-1.1-.3-.3-.4-.6-.4-1 0-.8.7-1.4 1.5-1.4H16a5 5 0 0 0 5-5c0-4.4-4-8-9-8Z', 'M7.5 11.5h.01M10.5 7.5h.01M15.5 8.5h.01']
} as const satisfies Record<string, readonly string[]>;

export type IconName = keyof typeof ICONS;

export const isIcon = (name: string): name is IconName => name in ICONS;

/** Token colour of a widget's drawer tile (DESIGN §6 "icon in a per-widget token colour"). */
export const ICON_TONE: Partial<Record<string, string>> = {
	clock: 'var(--k-accent)',
	calendar: 'var(--k-success)',
	cloud: 'var(--k-warning)',
	sun: 'var(--k-warning)',
	cpu: 'var(--k-info)',
	activity: 'var(--k-info)',
	todo: 'var(--k-success)',
	music: 'var(--k-accent-2)',
	agents: 'var(--k-accent-2)'
};
