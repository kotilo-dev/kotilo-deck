/**
 * Calendar feeds (global `calendarFeeds` setting) for names/colours and the empty state. Inside Tauri they
 * come from `settings_get` + `settings://changed`; in the browser preview from the localStorage settings.
 */
import { isTauri, on, settingsGet, type CalendarFeed } from '#lib/ipc/index.ts';
import { normalizeSettings } from '#lib/workspace/index.ts';

export async function loadFeeds(): Promise<CalendarFeed[]> {
	if (isTauri()) return (await settingsGet()).calendarFeeds ?? [];
	try {
		const raw = typeof localStorage === 'undefined' ? null : localStorage.getItem('kotilo-deck:settings');
		return normalizeSettings(raw ? JSON.parse(raw) : {}).calendarFeeds;
	} catch {
		return [];
	}
}

/** Feed list edits from the settings window. Returns a synchronous unsubscribe. */
export function watchFeeds(cb: (feeds: CalendarFeed[]) => void): () => void {
	let disposed = false;
	let off: (() => void) | null = null;
	on('settings://changed', (s) => cb(s.calendarFeeds ?? [])).then(
		(u) => (disposed ? u() : (off = u)),
		() => {}
	);
	return () => {
		disposed = true;
		off?.();
	};
}
