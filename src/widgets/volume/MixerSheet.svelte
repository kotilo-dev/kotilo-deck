<script lang="ts">
	/**
	 * Per-app mixer (SPEC §4.4 Volume, Phase 2): modal sheet listing `audioSessions` with a slider and a
	 * mute toggle each. Refreshed every 2 s while open and the deck is visible; touched rows stay local
	 * for HOLD_MS so a refresh doesn't snap the slider back mid-drag.
	 */
	import { t } from '#lib/i18n/index.ts';
	import { audioSessions, audioSetSessionMuted, audioSetSessionVolume, type AudioSession } from '#lib/ipc/index.ts';
	import { isDeckVisible, onDeckVisibility } from '#lib/time/index.ts';
	import { createThrottle, fromPct, mergeSessions, MIXER_REFRESH_MS, sortSessions, speakerLevel, toPct, type Throttle } from './logic';
	import SpeakerIcon from './SpeakerIcon.svelte';

	let dialog: HTMLDialogElement | undefined = $state();
	let sessions = $state<AudioSession[] | null>(null);
	let failed = $state(false);
	let timer: ReturnType<typeof setInterval> | null = null;
	let isOpen = false;
	const touchedAt = new Map<string, number>();
	const throttles = new Map<string, Throttle<number>>();
	const titleId = `vol-mix-${Math.random().toString(36).slice(2, 8)}`;

	let stopVisibility: (() => void) | null = null;

	function startPolling() {
		stopPolling();
		refresh();
		timer = setInterval(refresh, MIXER_REFRESH_MS);
	}

	function stopPolling() {
		if (timer !== null) clearInterval(timer);
		timer = null;
	}

	async function refresh() {
		try {
			const list = await audioSessions();
			if (!isOpen) return;
			sessions = sessions ? mergeSessions(list, sessions, touchedAt, Date.now()) : sortSessions(list);
			failed = false;
		} catch {
			if (isOpen) failed = true;
		}
	}

	export function open() {
		if (!dialog || dialog.open) return;
		isOpen = true;
		sessions = null;
		failed = false;
		dialog.showModal();
		// Poll only while the deck is on screen; a hidden deck has no interval at all.
		if (isDeckVisible()) startPolling();
		stopVisibility = onDeckVisibility((v) => (v ? startPolling() : stopPolling()));
	}

	function onclose() {
		isOpen = false;
		stopVisibility?.();
		stopVisibility = null;
		stopPolling();
		for (const th of throttles.values()) th.flush();
		throttles.clear();
		touchedAt.clear();
	}

	function throttleFor(id: string): Throttle<number> {
		let th = throttles.get(id);
		if (!th) {
			th = createThrottle((v: number) => {
				audioSetSessionVolume(id, v).catch(() => {});
			});
			throttles.set(id, th);
		}
		return th;
	}

	function patch(id: string, p: Partial<AudioSession>) {
		touchedAt.set(id, Date.now());
		sessions = sessions?.map((s) => (s.id === id ? { ...s, ...p } : s)) ?? null;
	}

	function onVolume(id: string, pct: number) {
		const v = fromPct(pct);
		patch(id, { volume: v });
		throttleFor(id).push(v);
	}

	function toggleMute(s: AudioSession) {
		const muted = !s.muted;
		patch(s.id, { muted });
		audioSetSessionMuted(s.id, muted).catch(() => {});
	}

	function onclick(e: MouseEvent) {
		if (e.target === dialog) dialog?.close();
	}

	$effect(() => () => {
		onclose();
		if (dialog?.open) dialog.close();
	});
</script>

<!-- The deck grid ignores gestures inside `dialog`, so slider drags here never become long-press/swipe. -->
<dialog bind:this={dialog} class="sheet" aria-labelledby={titleId} {onclose} {onclick}>
	<div class="panel">
		<header class="head">
			<h2 id={titleId}>{t('widgets.volume.mixerTitle')}</h2>
			<button type="button" class="close" aria-label={t('widgets.common.close')} onclick={() => dialog?.close()}>
				<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
					<path d="M18 6 6 18M6 6l12 12" />
				</svg>
			</button>
		</header>
		{#if failed && !sessions}
			<p class="note">{t('widgets.volume.sessionsFailed')}</p>
		{:else if sessions === null}
			<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
				{#each [0, 1, 2] as i (i)}<span class="bar"></span>{/each}
			</div>
		{:else if sessions.length === 0}
			<p class="note">{t('widgets.volume.noSessions')}</p>
		{:else}
			<ul class="list">
				{#each sessions as s (s.id)}
					{@const pct = toPct(s.volume)}
					<li class="item">
						<div class="top">
							<span class="name">{s.name}</span>
							<span class="pct" class:muted={s.muted}>{s.muted ? t('widgets.volume.muted') : t('widgets.volume.percent', { n: pct })}</span>
						</div>
						<div class="controls">
							<button
								type="button"
								class="mute"
								aria-pressed={s.muted}
								aria-label={s.muted ? t('widgets.volume.unmuteApp', { name: s.name }) : t('widgets.volume.muteApp', { name: s.name })}
								onclick={() => toggleMute(s)}
							>
								<SpeakerIcon level={speakerLevel(s.volume, s.muted)} />
							</button>
							<input
								type="range"
								class="range"
								min="0"
								max="100"
								step="1"
								value={pct}
								aria-label={t('widgets.volume.appVolume', { name: s.name })}
								aria-valuetext={s.muted ? t('widgets.volume.muted') : t('widgets.volume.percent', { n: pct })}
								oninput={(e) => onVolume(s.id, Number(e.currentTarget.value))}
								onchange={() => throttles.get(s.id)?.flush()}
							/>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</dialog>

<style>
	.sheet {
		margin: auto;
		width: min(460px, calc(100vw - 48px));
		max-height: calc(100vh - 48px);
		padding: 0;
		border: 1px solid var(--k-border);
		border-radius: var(--k-radius);
		background: var(--k-surface);
		color: var(--k-text);
		font-family: var(--k-font-ui);
	}
	.sheet::backdrop {
		background: color-mix(in srgb, var(--k-bg) 70%, transparent);
	}
	.panel {
		padding: 18px 20px 20px;
		display: flex;
		flex-direction: column;
		gap: 12px;
		max-height: calc(100vh - 50px);
		box-sizing: border-box;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}
	h2 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}
	.close,
	.mute {
		flex-shrink: 0;
		width: 44px;
		height: 44px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
	}
	.mute[aria-pressed='true'] {
		color: var(--k-text);
		background: var(--k-bg-alt);
	}
	.list {
		margin: 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-direction: column;
		overflow-y: auto;
		min-height: 0;
	}
	.item {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 10px 0;
		border-top: 1px solid var(--k-border);
	}
	.top {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		font-size: 13px;
	}
	.name {
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.pct {
		flex-shrink: 0;
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.pct.muted {
		font-family: var(--k-font-ui);
		color: var(--k-text-muted);
	}
	.controls {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.range {
		flex: 1;
		min-width: 0;
		height: 28px;
		margin: 0;
		accent-color: var(--k-accent);
	}
	.note {
		margin: 0;
		font-size: 13px;
		color: var(--k-text-muted);
	}
	.skeleton {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.bar {
		height: 52px;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}
</style>
