import { describe, expect, it } from 'vitest';
import en from './en.json';
import tr from './tr.json';

type Dict = { [key: string]: string | Dict };

/** "a.b.c" → string value, for every leaf. */
function leaves(dict: Dict, prefix = ''): Map<string, string> {
	const out = new Map<string, string>();
	for (const [k, v] of Object.entries(dict)) {
		const key = prefix ? `${prefix}.${k}` : k;
		if (typeof v === 'string') out.set(key, v);
		else for (const [kk, vv] of leaves(v, key)) out.set(kk, vv);
	}
	return out;
}

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe('i18n (SPEC cross-cutting: ship en and tr)', () => {
	const a = leaves(en as Dict);
	const b = leaves(tr as Dict);

	it('en and tr have exactly the same keys', () => {
		const missingInTr = [...a.keys()].filter((k) => !b.has(k));
		const missingInEn = [...b.keys()].filter((k) => !a.has(k));
		expect({ missingInTr, missingInEn }).toEqual({ missingInTr: [], missingInEn: [] });
	});

	it('every translation is non-empty and uses the same {placeholders}', () => {
		const bad: string[] = [];
		for (const [key, value] of a) {
			const other = b.get(key);
			if (other === undefined) continue;
			if (!value.trim() || !other.trim()) bad.push(`${key}: empty`);
			else if (placeholders(value).join() !== placeholders(other).join()) bad.push(`${key}: placeholders differ`);
		}
		expect(bad).toEqual([]);
	});
});
