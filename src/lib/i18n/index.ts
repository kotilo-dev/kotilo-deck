import en from '../../i18n/en.json';
import tr from '../../i18n/tr.json';

type Dict = { [key: string]: string | Dict };

const dicts: Record<string, Dict> = { en, tr };

/** OS/browser locale, used for all Intl formatting (SPEC cross-cutting i18n). */
export const locale: string = typeof navigator !== 'undefined' ? navigator.language : 'en';
const lang = locale.split('-')[0] in dicts ? locale.split('-')[0] : 'en';

function lookup(dict: Dict, key: string): string | undefined {
	let node: string | Dict | undefined = dict;
	for (const part of key.split('.')) {
		if (typeof node !== 'object') return undefined;
		node = node[part];
	}
	return typeof node === 'string' ? node : undefined;
}

/** `t('widgets.weather.stale')`, `t('shell.editing', { name })` → "{name}" placeholders. Falls back to en, then the key. */
export function t(key: string, params: Record<string, string | number> = {}): string {
	const s = lookup(dicts[lang], key) ?? lookup(dicts.en, key) ?? key;
	return s.replace(/\{(\w+)\}/g, (_, p) => String(params[p] ?? `{${p}}`));
}
