/**
 * Live companion server status (protocol.md) for the deck top bar and the settings window:
 * `companionStatus()` once, then `companion://status` pushes (also on a LAN tablet). Without a backend the status stays null.
 */
import { emitTo } from '@tauri-apps/api/event';
import { companionStatus, hasBackend, isRemote, isTauri, on, settingsOpen, type CompanionStatus } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';

export class CompanionWatch {
	status = $state<CompanionStatus | null>(null);

	/** Returns the stop function (usable from onMount / $effect). */
	start(): () => void {
		if (!hasBackend()) return () => {};
		let alive = true;
		companionStatus()
			.then((s) => alive && (this.status = s))
			.catch(() => {});
		const off = on('companion://status', (s) => (this.status = s));
		return () => {
			alive = false;
			void off.then((f) => f());
		};
	}
}

/** off = disabled by the user; error = enabled but not listening (e.g. port taken); waiting = no desktop yet. */
export type CompanionView = 'off' | 'connected' | 'waiting' | 'error';

/** `enabled` is the user's setting: it wins over a status that hasn't caught up yet. */
export function companionView(enabled: boolean, status: CompanionStatus | null): CompanionView {
	if (!enabled) return 'off';
	if (status?.connected) return 'connected';
	if (status && status.enabled && !status.listening && status.error) return 'error';
	return 'waiting';
}

/** Top-bar label (DESIGN §4.2) and a longer tooltip. */
export function companionText(view: CompanionView, status: CompanionStatus | null): { label: string; title: string } {
	switch (view) {
		case 'off':
			return { label: t('shell.topbar.companionDisabled'), title: t('shell.topbar.companionDisabled') };
		case 'connected': {
			const label = t('shell.topbar.companionOn');
			return { label, title: status?.client ? t('shell.topbar.companionVersion', { version: status.client }) : label };
		}
		case 'error':
			return { label: t('shell.topbar.companionOff'), title: status?.error ?? t('shell.topbar.companionOff') };
		default:
			return { label: t('shell.topbar.companionOff'), title: t('shell.topbar.companionOff') };
	}
}

/** localStorage hand-off for a settings window that is still being created (it reads this on mount). */
export const SETTINGS_SECTION_KEY = 'kotilo-deck:settings-section';
/** Frontend-only event to an already open settings window. */
export const SETTINGS_NAVIGATE_EVENT = 'settings://navigate';

/** Settings can be opened from here (not on a LAN tablet/browser: settings stay on the host). */
export const canOpenSettings = (): boolean => !isRemote();

/** Opens the settings window, optionally at a section (e.g. "companion"). No-op in remote mode. */
export async function openSettingsAt(section?: string): Promise<void> {
	if (isRemote()) return;
	if (!isTauri()) {
		window.open(section ? `/settings#${section}` : '/settings', 'kotilo-deck-settings', 'width=960,height=680');
		return;
	}
	if (section) {
		try {
			localStorage.setItem(SETTINGS_SECTION_KEY, JSON.stringify({ section, at: Date.now() }));
		} catch {
			// Storage unavailable: the event below still reaches an open window.
		}
	}
	await settingsOpen();
	if (section) await emitTo('settings', SETTINGS_NAVIGATE_EVENT, { section }).catch(() => {});
}

/** Reads (and clears) a section hand-off written in the last 10 s. */
export function takePendingSection(): string | null {
	try {
		const raw = localStorage.getItem(SETTINGS_SECTION_KEY);
		if (!raw) return null;
		localStorage.removeItem(SETTINGS_SECTION_KEY);
		const v = JSON.parse(raw) as { section?: unknown; at?: unknown };
		return typeof v.section === 'string' && typeof v.at === 'number' && Date.now() - v.at < 10_000 ? v.section : null;
	} catch {
		return null;
	}
}
