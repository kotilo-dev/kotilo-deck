/**
 * First-run onboarding (SPEC §8 Phase 5): pure step machine and settings patches. The overlay itself is
 * `Onboarding.svelte`; everything here is testable without a DOM.
 */
import type { DeckSettings, Workspace } from '#lib/ipc/index.ts';
import { PRESET_IDS } from '#lib/workspace/presets.ts';

/** Onboarding flow version written on completion (DeckSettings.onboarding.version). */
export const ONBOARDING_VERSION = 1;

/** Canonical order; the visible list drops steps that don't apply (one monitor). */
export const ALL_STEPS = ['welcome', 'monitor', 'theme', 'workspace', 'done'] as const;
export type StepId = (typeof ALL_STEPS)[number];

/** URL flag that shows onboarding outside Tauri (demos, screenshots): `/?onboarding`. */
export const FORCE_PARAM = 'onboarding';

export interface ShowInput {
	/** LAN tablet/browser: never (settings live on the host). */
	remote: boolean;
	/** Inside the Tauri host window (backend present). */
	tauri: boolean;
	/** `location.search`. */
	search: string;
	completed: boolean;
	/**
	 * Finished or skipped and the `completed: true` write hasn't been seen yet (or failed). Keeps a failed
	 * write from reopening the overlay; the page clears it once settings report completed.
	 */
	dismissed: boolean;
}

/**
 * Host: whenever `onboarding.completed` is false (also after "Run setup again"). Plain browser without a
 * backend: only with `?onboarding`, once per page load. Remote: never.
 */
export function shouldShowOnboarding({ remote, tauri, search, completed, dismissed }: ShowInput): boolean {
	if (remote || dismissed) return false;
	if (tauri) return !completed;
	try {
		return new URLSearchParams(search).has(FORCE_PARAM);
	} catch {
		return false;
	}
}

/** Visible steps: the monitor step only when there is a real choice (2+ monitors). */
export function onboardingSteps(monitorCount: number): StepId[] {
	return ALL_STEPS.filter((s) => s !== 'monitor' || monitorCount > 1);
}

const order = (s: StepId) => ALL_STEPS.indexOf(s);

/**
 * Next visible step after `current` (by canonical order, so it still works if `current` just dropped out
 * of the list, e.g. a monitor was unplugged while its step showed). null = past the end.
 */
export function nextStep(steps: readonly StepId[], current: StepId): StepId | null {
	return steps.find((s) => order(s) > order(current)) ?? null;
}

/** Previous visible step, or null on the first one. */
export function prevStep(steps: readonly StepId[], current: StepId): StepId | null {
	return [...steps].reverse().find((s) => order(s) < order(current)) ?? null;
}

/** The step to show: `current` if visible, else the next visible one (else the last). */
export function visibleStep(steps: readonly StepId[], current: StepId): StepId {
	if (steps.includes(current)) return current;
	return nextStep(steps, current) ?? steps[steps.length - 1];
}

/** Written on "Open deck", "Skip setup" and Esc alike: skipping also counts as done. */
export const completionPatch = (): Pick<DeckSettings, 'onboarding'> => ({
	onboarding: { completed: true, version: ONBOARDING_VERSION }
});

/** Settings ▸ About ▸ "Run setup again": the deck shows onboarding on the next settings://changed. */
export const resetPatch = (current: DeckSettings['onboarding']): Pick<DeckSettings, 'onboarding'> => ({
	onboarding: { completed: false, version: current.version }
});

/**
 * Workspace step choices: the bundled presets (Focus / Ops / Agents / Full) as seeded on first run, in
 * preset order. A preset whose workspace is gone is left out; if none exist, the first four workspaces.
 */
export function workspaceChoices(workspaces: readonly Workspace[]): Workspace[] {
	const presets = PRESET_IDS.map((id) => workspaces.find((w) => w.id === id)).filter((w): w is Workspace => !!w);
	return presets.length > 0 ? presets : workspaces.slice(0, 4);
}
