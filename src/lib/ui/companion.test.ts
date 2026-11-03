import { describe, expect, it } from 'vitest';
import type { CompanionStatus } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';
import { companionText, companionView } from './companion.svelte';

const status = (p: Partial<CompanionStatus> = {}): CompanionStatus => ({
	enabled: true,
	listening: true,
	connected: false,
	client: null,
	port: 47831,
	error: null,
	...p
});

describe('companion status (DESIGN §4.2)', () => {
	it('derives the view from the setting first, then the live status', () => {
		expect(companionView(false, status({ connected: true }))).toBe('off');
		expect(companionView(true, null)).toBe('waiting');
		expect(companionView(true, status())).toBe('waiting');
		expect(companionView(true, status({ connected: true, client: '1.4.0' }))).toBe('connected');
		expect(companionView(true, status({ listening: false, error: 'port 47831 in use' }))).toBe('error');
		// Status still says disabled right after the user turned it on: not an error yet.
		expect(companionView(true, status({ enabled: false, listening: false, error: 'x' }))).toBe('waiting');
	});

	it('labels: "Kotilo desktop" when connected (version in the tooltip), "not running" otherwise, never the error as label', () => {
		expect(companionText('connected', status({ connected: true, client: '1.4.0' }))).toEqual({
			label: t('shell.topbar.companionOn'),
			title: t('shell.topbar.companionVersion', { version: '1.4.0' })
		});
		expect(companionText('waiting', status()).label).toBe(t('shell.topbar.companionOff'));
		expect(companionText('error', status({ listening: false, error: 'port in use' }))).toEqual({
			label: t('shell.topbar.companionOff'),
			title: 'port in use'
		});
		expect(companionText('off', null).label).toBe(t('shell.topbar.companionDisabled'));
	});
});
