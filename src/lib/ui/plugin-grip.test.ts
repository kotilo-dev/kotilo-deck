import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { GRIP_PX } from '#lib/plugins/PluginFrame.svelte';

/**
 * Long-press on a plugin card (DESIGN §6): the iframe swallows pointer events, so PluginFrame keeps a
 * transparent grip ring on the card edges outside edit mode. It must be wide enough to hit and stay
 * inside the smallest card padding so it never covers the plugin's own controls.
 */
describe('plugin card grip ring', () => {
	it('stays inside the smallest card padding (DESIGN §3: 14 px on 1-row cards)', () => {
		expect(GRIP_PX).toBeGreaterThanOrEqual(8);
		expect(GRIP_PX).toBeLessThan(14);
	});

	it('is only rendered outside edit mode, where the full overlay takes over', () => {
		const src = readFileSync(new URL('../plugins/PluginFrame.svelte', import.meta.url), 'utf8');
		const block = src.slice(src.indexOf('{#if editing}'), src.indexOf('{/if}', src.indexOf('class="grip"')));
		expect(block).toMatch(/class="overlay"[\s\S]*\{:else\}[\s\S]*class="grip"/);
	});
});
