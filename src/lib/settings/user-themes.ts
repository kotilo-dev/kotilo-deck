/** Loads `~/.kotilo/themes/*.json` through the backend and swaps the user layer of the theme registry. */
import { isTauri, themeUserList } from '#lib/ipc/index.ts';
import { parseUserThemes, setUserThemes } from '#lib/theme/index.ts';

/**
 * Never throws: an unreadable folder keeps the previous user themes; invalid files are skipped and
 * returned as `errors` (also logged) so the caller can show a toast.
 */
export async function refreshUserThemes(): Promise<{ label: string; message: string }[]> {
	if (!isTauri()) return [];
	let raw: unknown[];
	try {
		raw = await themeUserList();
	} catch (e) {
		console.warn('kotilo-deck: user themes unavailable', e);
		return [];
	}
	const { themes, errors } = parseUserThemes(Array.isArray(raw) ? raw : []);
	for (const e of errors) console.warn(`kotilo-deck: skipped user theme ${e.label}: ${e.message}`);
	setUserThemes(themes, errors);
	return errors;
}
