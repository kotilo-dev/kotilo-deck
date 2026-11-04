<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import {
		audioSetDefaultDevice,
		audioSetMuted,
		audioSetVolume,
		audioState,
		audioSubscribe,
		audioUnsubscribe,
		hasBackend,
		on,
		type AudioState
	} from '#lib/ipc/index.ts';
	import { isDeckVisible, onDeckVisibility } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import { createOptimistic, createThrottle, deviceRows, fromPct, speakerLevel, toPct } from './logic';
	import MixerSheet from './MixerSheet.svelte';
	import { defaults, type VolumeSettings } from './settings';
	import SpeakerIcon from './SpeakerIcon.svelte';

	let { size, settings, editing, available }: WidgetProps<VolumeSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const tauri = hasBackend();
	const uid = `vol-${Math.random().toString(36).slice(2, 9)}`;
	const menuId = `${uid}-menu`;
	/** DESIGN §5.6: device radio rows are 40 px. */
	const DEVICE_ROW = 40;

	let audio = $state<AudioState | null>(null);
	let failed = $state(false);
	let retryKey = $state(0);
	/** Bumped when an optimistic value is set or settles, so the derived values re-read it. */
	let bump = $state(0);

	const vol = createOptimistic<number>(() => bump++);
	const mute = createOptimistic<boolean>(() => bump++);
	const dev = createOptimistic<string | null>(() => bump++, 1500);
	const sendVolume = createThrottle((v: number) => {
		audioSetVolume(v).catch(() => {});
	});

	let visible = $state(isDeckVisible());
	$effect(() => onDeckVisibility((v) => (visible = v)));

	// Backend pushes `audio://state` from OS notifications while subscribed (ref-counted). Hidden deck → unsubscribe.
	$effect(() => {
		void retryKey;
		if (!tauri || !available || !visible) return;
		let disposed = false;
		let unlisten: (() => void) | null = null;
		on('audio://state', (st) => {
			if (disposed) return;
			audio = st;
			failed = false;
		})
			.then((u) => (disposed ? u() : (unlisten = u)))
			.catch(() => {});
		const subscribed = audioSubscribe().then(
			() => true,
			() => false
		);
		audioState()
			.then((st) => {
				if (disposed) return;
				audio = st;
				failed = false;
			})
			.catch(() => {
				if (!disposed && !audio) failed = true;
			});
		return () => {
			disposed = true;
			unlisten?.();
			subscribed.then((ok) => {
				if (ok) audioUnsubscribe().catch(() => {});
			});
		};
	});

	$effect(() => () => {
		sendVolume.flush();
		vol.dispose();
		mute.dispose();
		dev.dispose();
	});

	const pct = $derived.by(() => {
		void bump;
		return toPct(vol.value(audio?.volume ?? 0));
	});
	const muted = $derived.by(() => {
		void bump;
		return mute.value(audio?.muted ?? false);
	});
	const defaultId = $derived.by(() => {
		void bump;
		return dev.value(audio?.device?.id ?? null);
	});
	const devices = $derived(audio?.devices ?? []);
	const deviceName = $derived(devices.find((d) => d.id === defaultId)?.name ?? audio?.device?.name ?? '');
	const hasDevice = $derived(!!audio?.device || devices.length > 0);
	const level = $derived(speakerLevel(pct / 100, muted));
	const valueText = $derived(muted ? t('widgets.volume.muted') : t('widgets.volume.percent', { n: pct }));

	function onInput(e: Event & { currentTarget: HTMLInputElement }) {
		const v = fromPct(Number(e.currentTarget.value));
		vol.set(v);
		bump++;
		sendVolume.push(v);
	}

	// The deck grid ignores gestures on input[type=range] and [popover] (no long-press / swipe there).
	function onRangeDown() {
		vol.hold(true);
	}

	function onRangeUp() {
		vol.hold(false);
		sendVolume.flush();
	}

	function toggleMute() {
		if (editing) return;
		const next = !muted;
		mute.set(next);
		bump++;
		audioSetMuted(next).catch(() => {});
	}

	let menu: HTMLDivElement | undefined = $state();
	let trigger: HTMLElement | null = null;

	function pickDevice(id: string) {
		menu?.hidePopover?.();
		if (editing || id === defaultId) return;
		dev.set(id);
		bump++;
		audioSetDefaultDevice(id).catch(() => {});
	}

	function placeMenu(e: Event) {
		const ev = e as ToggleEvent;
		if (ev.newState !== 'open' || !menu || !trigger) return;
		const r = trigger.getBoundingClientRect();
		const width = Math.max(220, Math.min(320, r.width + 80));
		const left = Math.max(8, Math.min(window.innerWidth - width - 8, r.left));
		const below = window.innerHeight - r.bottom - 16;
		menu.style.width = `${width}px`;
		menu.style.left = `${left}px`;
		if (below >= 160 || below >= r.top) {
			menu.style.top = `${r.bottom + 6}px`;
			menu.style.bottom = '';
			menu.style.maxHeight = `${below}px`;
		} else {
			menu.style.top = '';
			menu.style.bottom = `${window.innerHeight - r.top + 6}px`;
			menu.style.maxHeight = `${r.top - 16}px`;
		}
	}

	/** Menu keyboard: focus the current device on open; arrows / Home / End move between items. */
	function focusMenu(e: Event) {
		if ((e as ToggleEvent).newState !== 'open' || !menu) return;
		(menu.querySelector<HTMLElement>('[aria-checked="true"]') ?? menu.querySelector<HTMLElement>('button'))?.focus();
	}

	function menuKey(e: KeyboardEvent) {
		if (!menu || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
		e.preventDefault();
		const items = [...menu.querySelectorAll<HTMLElement>('button')];
		const i = items.indexOf(document.activeElement as HTMLElement);
		const n = items.length;
		const next = e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : (i + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
		items[next]?.focus();
	}

	let listH = $state(0);
	const slots = $derived(Math.max(0, Math.floor(listH / DEVICE_ROW)));
	const rows = $derived(deviceRows(devices, defaultId, slots));

	let mixer: MixerSheet | undefined = $state();
</script>

{#snippet chevron()}
	<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="m6 9 6 6 6-6" />
	</svg>
{/snippet}

{#snippet mixerIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
	</svg>
{/snippet}

{#snippet top()}
	<div class="top">
		<button
			type="button"
			class="icon-btn"
			aria-pressed={muted}
			aria-label={muted ? t('widgets.volume.unmute') : t('widgets.volume.mute')}
			disabled={editing || !hasDevice}
			onclick={toggleMute}
		>
			<SpeakerIcon {level} />
		</button>
		<span class="name">{hasDevice ? deviceName || t('widgets.volume.title') : t('widgets.volume.noDevice')}</span>
		{#if hasDevice}
			{#if muted}
				<span class="pct muted-text">{t('widgets.volume.muted')}</span>
			{:else}
				<span class="pct mono">{t('widgets.volume.percent', { n: pct })}</span>
			{/if}
		{/if}
	</div>
	<input
		type="range"
		class="range"
		min="0"
		max="100"
		step="1"
		value={pct}
		aria-label={t('widgets.volume.volume')}
		aria-valuetext={valueText}
		disabled={editing || !hasDevice}
		oninput={onInput}
		onchange={onRangeUp}
		onpointerdown={onRangeDown}
		onpointerup={onRangeUp}
		onpointercancel={onRangeUp}
	/>
{/snippet}

{#snippet deviceMenu()}
	<div
		bind:this={menu}
		id={menuId}
		popover="auto"
		class="k-popover menu"
		role="menu"
		tabindex="-1"
		aria-label={t('widgets.volume.outputDevices')}
		onbeforetoggle={placeMenu}
		ontoggle={focusMenu}
		onkeydown={menuKey}
	>
		{#each devices as d (d.id)}
			<button
				type="button"
				class="k-menu-item"
				role="menuitemradio"
				aria-checked={d.id === defaultId}
				onclick={() => pickDevice(d.id)}
			>
				<span class="tick" aria-hidden="true">
					{#if d.id === defaultId}
						<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5" /></svg>
					{/if}
				</span>
				<span class="menu-name">{d.name}</span>
			</button>
		{/each}
	</div>
{/snippet}

{#snippet stateIcon()}
	<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M11 5 6 9H2v6h4l5 4V5Z" />
		<path d="m22 9-6 6M16 9l6 6" />
	</svg>
{/snippet}

<section class="k-card volume" data-size={size} aria-label={t('widgets.volume.title')}>
	{#if !tauri || !available}
		<div class="state">
			{@render stateIcon()}
			<span>{!tauri ? t('widgets.common.browserPreview') : t('widgets.common.notAvailable')}</span>
		</div>
	{:else if failed && !audio}
		<div class="state">
			{@render stateIcon()}
			<span class="state-title">{t('widgets.volume.error')}</span>
			{#if size === '2x2'}
				<button type="button" class="k-btn" disabled={editing} onclick={() => retryKey++}>{t('widgets.common.retry')}</button>
			{:else}
				<button type="button" class="ghost" disabled={editing} onclick={() => retryKey++}>{t('widgets.common.retry')}</button>
			{/if}
		</div>
	{:else if !audio}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar" style="width: 70%; height: 16px"></span>
			<span class="bar" style="height: 20px"></span>
			{#if size === '2x2'}<span class="bar" style="flex: 1"></span>{/if}
		</div>
	{:else if size === '2x2'}
		{@render top()}
		{#if s.devicePicker}
			<fieldset class="devices" bind:clientHeight={listH} disabled={editing}>
				<legend class="k-sr-only">{t('widgets.volume.outputDevices')}</legend>
				{#each rows.rows as d (d.id)}
					<label class="dev">
						<input type="radio" name={`${uid}-dev`} value={d.id} checked={d.id === defaultId} onchange={() => pickDevice(d.id)} />
						<span class="dev-name">{d.name}</span>
					</label>
				{/each}
				{#if rows.more}
					<button type="button" class="dev more" popovertarget={menuId} onclick={(e) => (trigger = e.currentTarget)}>
						<span class="dev-name">{t('widgets.volume.moreDevices')}</span>
						{@render chevron()}
					</button>
				{/if}
			</fieldset>
		{:else}
			<div class="spacer"></div>
		{/if}
		<div class="foot">
			<button type="button" class="k-btn foot-btn" aria-pressed={muted} disabled={editing || !hasDevice} onclick={toggleMute}>
				<SpeakerIcon level={muted ? 2 : 0} size={16} />
				{muted ? t('widgets.volume.unmute') : t('widgets.volume.mute')}
			</button>
			{#if s.mixer}
				<button type="button" class="k-btn foot-btn" aria-haspopup="dialog" disabled={editing} onclick={() => mixer?.open()}>
					{@render mixerIcon()}
					{t('widgets.volume.mixer')}
				</button>
			{/if}
		</div>
		{@render deviceMenu()}
	{:else}
		{@render top()}
		{#if (s.devicePicker && devices.length > 1) || s.mixer}
			<div class="actions">
				{#if s.devicePicker && devices.length > 1}
					<button
						type="button"
						class="ghost"
						popovertarget={menuId}
						aria-haspopup="menu"
						disabled={editing}
						onclick={(e) => (trigger = e.currentTarget)}
					>
						{t('widgets.volume.outputDevice')}
						{@render chevron()}
					</button>
				{/if}
				{#if s.mixer}
					<button type="button" class="ghost" aria-haspopup="dialog" disabled={editing} onclick={() => mixer?.open()}>
						{@render mixerIcon()}
						{t('widgets.volume.mixer')}
					</button>
				{/if}
			</div>
		{/if}
		{@render deviceMenu()}
	{/if}
	{#if tauri && available}
		<MixerSheet bind:this={mixer} />
	{/if}
</section>

<style>
	.volume {
		container-type: size;
		padding: 18px 20px;
		gap: 10px;
		font-family: var(--k-font-ui);
	}
	.volume[data-size='2x1'] {
		padding: 14px 18px;
		justify-content: center;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.top {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	/* Speaker doubles as the mute toggle; the 2×2 footer has the full-size one. */
	.icon-btn {
		position: relative;
		flex-shrink: 0;
		width: 28px;
		height: 28px;
		margin: -5px 0 -5px -5px;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		border: 0;
		border-radius: 8px;
		background: transparent;
		color: var(--k-text);
	}
	/* Visual stays 28 px; the transparent ring makes the hit area 44×44 (DESIGN §9). */
	.icon-btn::before {
		content: '';
		position: absolute;
		inset: -8px;
	}
	.icon-btn[aria-pressed='true'] {
		color: var(--k-text-muted);
	}
	.name {
		flex: 1;
		min-width: 0;
		font-size: 13px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.pct {
		flex-shrink: 0;
		font-size: 13px;
	}
	.muted-text {
		color: var(--k-text-muted);
	}
	.range {
		width: 100%;
		height: 28px;
		margin: 0;
		flex-shrink: 0;
		accent-color: var(--k-accent);
		touch-action: none;
	}

	/* 2×1 ghost buttons, 28 px (DESIGN §5.6). */
	.actions {
		display: flex;
		gap: 8px;
		min-width: 0;
	}
	.ghost {
		height: 28px;
		padding: 0 10px;
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		border-radius: 8px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
		font-size: 11px;
		white-space: nowrap;
	}
	.ghost:hover:not(:disabled) {
		color: var(--k-text);
	}

	/* 2×2 */
	.devices {
		flex: 1;
		min-height: 0;
		min-width: 0;
		margin: 0;
		padding: 0;
		border: 0;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}
	.dev {
		box-sizing: border-box;
		flex-shrink: 0;
		height: 40px;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 0 10px;
		border-radius: 10px;
		font-size: 13px;
		cursor: pointer;
		min-width: 0;
	}
	.dev:has(input:checked) {
		background: var(--k-bg-alt);
		font-weight: 600;
	}
	.dev input {
		flex-shrink: 0;
		width: 16px;
		height: 16px;
		margin: 0;
		accent-color: var(--k-accent);
	}
	.dev-name {
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		text-align: left;
	}
	.dev.more {
		width: 100%;
		border: 0;
		background: transparent;
		color: var(--k-text-muted);
	}
	.spacer {
		flex: 1;
	}
	.foot {
		display: flex;
		gap: 8px;
		flex-shrink: 0;
	}
	.foot-btn {
		flex: 1;
		min-width: 0;
		padding: 0 10px;
	}
	.foot-btn[aria-pressed='true'] {
		background: var(--k-bg-alt);
	}

	.menu {
		display: none;
		flex-direction: column;
		gap: 2px;
		overflow-y: auto;
	}
	.menu:popover-open {
		display: flex;
	}
	.tick {
		flex-shrink: 0;
		width: 16px;
		height: 16px;
		color: var(--k-accent);
	}
	.menu-name {
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		text-align: center;
		font-size: 12px;
		color: var(--k-text-muted);
		min-height: 0;
	}
	.volume[data-size='2x1'] .state {
		flex-direction: row;
		gap: 10px;
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--k-text);
	}
	.skeleton {
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: 10px;
		min-height: 0;
	}
	.bar {
		display: block;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}

	/* 1024×600: a 2×1 cell is ~112 px tall — tighten the stack, keep all three rows. */
	@container (height < 100px) {
		.volume[data-size='2x1'] {
			gap: 4px;
		}
	}
	/* Narrow 2×1: icon-only Mixer button. */
	@container (width < 230px) {
		.volume[data-size='2x1'] .actions .ghost:last-child:not(:first-child) {
			font-size: 0;
			gap: 0;
		}
	}
</style>
