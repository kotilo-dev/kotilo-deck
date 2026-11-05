import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS, makePreset, memoryStorage, normalizeSettings, storageBackend } from '#lib/workspace/index.ts';
import {
	completionPatch,
	nextStep,
	ONBOARDING_VERSION,
	onboardingSteps,
	prevStep,
	resetPatch,
	shouldShowOnboarding,
	visibleStep,
	workspaceChoices,
	type ShowInput
} from './index.ts';

const host: ShowInput = { remote: false, tauri: true, search: '', completed: false, dismissed: false };

describe('shouldShowOnboarding', () => {
	it('shows on the host until completed', () => {
		expect(shouldShowOnboarding(host)).toBe(true);
		expect(shouldShowOnboarding({ ...host, completed: true })).toBe(false);
	});

	it('never shows on a LAN remote, even when not completed or forced', () => {
		expect(shouldShowOnboarding({ ...host, remote: true, tauri: false })).toBe(false);
		expect(shouldShowOnboarding({ ...host, remote: true, tauri: false, search: '?onboarding' })).toBe(false);
	});

	it('plain browser: only with ?onboarding, once per session', () => {
		const browser = { ...host, tauri: false };
		expect(shouldShowOnboarding(browser)).toBe(false);
		expect(shouldShowOnboarding({ ...browser, search: '?onboarding' })).toBe(true);
		expect(shouldShowOnboarding({ ...browser, search: '?x=1&onboarding=1' })).toBe(true);
		expect(shouldShowOnboarding({ ...browser, search: '?onboarding', completed: true })).toBe(true);
		expect(shouldShowOnboarding({ ...browser, search: '?onboarding', dismissed: true })).toBe(false);
	});

	it('a dismissed session stays closed on the host even if the write failed', () => {
		expect(shouldShowOnboarding({ ...host, dismissed: true })).toBe(false);
	});
});

describe('step machine', () => {
	it('auto-skips the monitor step with one (or unknown) monitor', () => {
		expect(onboardingSteps(0)).toEqual(['welcome', 'theme', 'workspace', 'done']);
		expect(onboardingSteps(1)).toEqual(['welcome', 'theme', 'workspace', 'done']);
		expect(onboardingSteps(2)).toEqual(['welcome', 'monitor', 'theme', 'workspace', 'done']);
	});

	it('walks forward and back over the visible steps', () => {
		const one = onboardingSteps(1);
		expect(nextStep(one, 'welcome')).toBe('theme');
		expect(prevStep(one, 'theme')).toBe('welcome');
		expect(prevStep(one, 'welcome')).toBeNull();
		expect(nextStep(one, 'done')).toBeNull();
		const two = onboardingSteps(2);
		expect(nextStep(two, 'welcome')).toBe('monitor');
		expect(nextStep(two, 'monitor')).toBe('theme');
		expect(prevStep(two, 'theme')).toBe('monitor');
	});

	it('moves on when the current step disappears (monitor unplugged)', () => {
		const one = onboardingSteps(1);
		expect(visibleStep(one, 'monitor')).toBe('theme');
		expect(nextStep(one, 'monitor')).toBe('theme');
		expect(prevStep(one, 'monitor')).toBe('welcome');
		expect(visibleStep(one, 'workspace')).toBe('workspace');
	});
});

describe('settings patches', () => {
	it('skip and finish both write completed with the flow version', () => {
		expect(completionPatch()).toEqual({ onboarding: { completed: true, version: ONBOARDING_VERSION } });
		expect(ONBOARDING_VERSION).toBe(1);
	});

	it('"Run setup again" only clears completed', () => {
		expect(resetPatch({ completed: true, version: 1 })).toEqual({ onboarding: { completed: false, version: 1 } });
	});

	it('the patch round-trips through the browser backend', async () => {
		const backend = storageBackend(memoryStorage());
		expect((await backend.getSettings()).onboarding.completed).toBe(false);
		await backend.setSettings(completionPatch());
		expect((await backend.getSettings()).onboarding).toEqual({ completed: true, version: 1 });
		await backend.setSettings(resetPatch({ completed: true, version: 1 }));
		expect((await backend.getSettings()).onboarding).toEqual({ completed: false, version: 1 });
	});
});

describe('normalizeSettings onboarding default', () => {
	it('fresh install (nothing stored) is not completed', () => {
		expect(normalizeSettings({}).onboarding).toEqual({ completed: false, version: 0 });
		expect(normalizeSettings(null).onboarding).toEqual(DEFAULT_SETTINGS.onboarding);
	});

	it('settings from an older version without the key count as completed', () => {
		expect(normalizeSettings({ theme: 'nord' }).onboarding).toEqual({ completed: true, version: 0 });
	});

	it('keeps stored values and repairs bad ones', () => {
		expect(normalizeSettings({ onboarding: { completed: false, version: 1 } }).onboarding).toEqual({ completed: false, version: 1 });
		expect(normalizeSettings({ theme: null, onboarding: { completed: 'yes' } }).onboarding).toEqual({ completed: true, version: 0 });
	});
});

describe('workspaceChoices', () => {
	it('lists the seeded presets in preset order', () => {
		const ws = [makePreset('full'), makePreset('focus'), { ...makePreset('focus'), id: 'mine', name: 'Mine' }, makePreset('ops')];
		expect(workspaceChoices(ws).map((w) => w.id)).toEqual(['focus', 'ops', 'full']);
	});

	it('falls back to the first four workspaces when no preset is left', () => {
		const ws = ['a', 'b', 'c', 'd', 'e'].map((id) => ({ ...makePreset('focus'), id }));
		expect(workspaceChoices(ws).map((w) => w.id)).toEqual(['a', 'b', 'c', 'd']);
	});
});
