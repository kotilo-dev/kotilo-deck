// Tailwind 4 scans every source file for class candidates and emits a global utility for each one it
// recognises. A component's own `class="ring"` therefore also picks up Tailwind's `.ring`
// (a 1 px box-shadow), `class="fixed"` picks up `position: fixed`, and so on — for every property the
// scoped style doesn't set itself. Custom class names must never be Tailwind utility names.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, relative, resolve } from 'node:path';
import { compile } from 'tailwindcss';
import { describe, expect, it } from 'vitest';

const ROOT = resolve(import.meta.dirname, '..', '..', '..');
const TW = dirname(createRequire(join(ROOT, 'package.json')).resolve('tailwindcss/package.json'));

function svelteFiles(dir: string, out: string[] = []): string[] {
	for (const name of readdirSync(dir)) {
		const p = join(dir, name);
		if (statSync(p).isDirectory()) svelteFiles(p, out);
		else if (p.endsWith('.svelte')) out.push(p);
	}
	return out;
}

/** Static names from `class="…"` (interpolations dropped) and `class:name` directives. */
function classNames(source: string): Set<string> {
	const names = new Set<string>();
	for (const m of source.matchAll(/\bclass="([^"]*)"/g)) {
		for (const c of m[1].replace(/\{[^}]*\}/g, ' ').split(/\s+/)) if (c) names.add(c);
	}
	for (const m of source.matchAll(/\bclass:([\w-]+)/g)) names.add(m[1]);
	return names;
}

async function tailwindUtilities(candidates: string[]): Promise<Set<string>> {
	const compiler = await compile("@import 'tailwindcss';", {
		base: ROOT,
		loadStylesheet: async (id: string) => {
			const file = id === 'tailwindcss' ? 'index.css' : id.replace(/^(\.\/|tailwindcss\/)/, '');
			const path = join(TW, file);
			return { path, base: TW, content: readFileSync(path, 'utf8') };
		}
	});
	const css = compiler.build(candidates);
	return new Set([...css.matchAll(/^\s*\.([\w-]+)\s*\{/gm)].map((m) => m[1].replace(/\\/g, '')));
}

describe('custom class names vs Tailwind utilities', () => {
	it('extracts static and directive class names', () => {
		const names = classNames('<div class="a {x} b" class:c={y}><span class="d"></span></div>');
		expect([...names].sort()).toEqual(['a', 'b', 'c', 'd']);
	});

	it('no .svelte file uses a Tailwind utility name as a bare class', async () => {
		const byName = new Map<string, string[]>();
		for (const file of svelteFiles(join(ROOT, 'src'))) {
			for (const c of classNames(readFileSync(file, 'utf8'))) {
				byName.set(c, [...(byName.get(c) ?? []), relative(ROOT, file).replace(/\\/g, '/')]);
			}
		}
		const utilities = await tailwindUtilities([...byName.keys()]);
		// Sanity: the probe really recognises the names that bit us.
		expect([...(await tailwindUtilities(['ring', 'fixed', 'grow', 'k-card']))].sort()).toEqual(['fixed', 'grow', 'ring']);
		const clashes = [...byName].filter(([c]) => utilities.has(c)).map(([c, files]) => `${c}: ${files.join(', ')}`);
		expect(clashes).toEqual([]);
	});
});
