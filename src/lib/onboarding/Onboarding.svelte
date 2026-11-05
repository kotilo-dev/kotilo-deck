<script lang="ts">
	/**
	 * First-run onboarding (SPEC §8 Phase 5): a full-screen modal over the deck. Welcome → monitor (only
	 * with 2+ monitors) → theme → workspace → done. Every choice applies live; "Skip setup", Esc and
	 * "Open deck" all write `onboarding.completed: true`. A modal <dialog> keeps focus inside and makes
	 * the deck's own shortcuts (E, Esc) stand down while it's open.
	 */
	import { onMount, tick } from 'svelte';
	import { fade } from 'svelte/transition';
	import { isTauri, monitorList, monitorSelect, type MonitorInfo, type Workspace } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { DEFAULT_THEME_ID, getTheme, themes } from '#lib/theme/index.ts';
	import { isPresetId } from '#lib/workspace/presets.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import Mark from '#lib/ui/Mark.svelte';
	import { iconTone } from '#lib/ui/icons.ts';
	import { definition, getDeck } from '#lib/ui/deck.svelte.ts';
	import { completionPatch, nextStep, onboardingSteps, prevStep, visibleStep, workspaceChoices, type StepId } from './index.ts';

	/** Called once with the pending `onboarding.completed: true` write. */
	let { ondone }: { ondone: (saved: Promise<void>) => void } = $props();

	const { store, ui } = getDeck();

	let dialog = $state<HTMLDialogElement>();
	let heading = $state<HTMLElement>();
	let current = $state<StepId>('welcome');
	let monitors = $state<MonitorInfo[] | null>(null);
	let monitorError = $state<string | null>(null);
	let pickedMonitor = $state<string | null>(null);
	let finished = false;

	const steps = $derived(onboardingSteps(monitors?.length ?? 0));
	const step = $derived(visibleStep(steps, current));
	const index = $derived(steps.indexOf(step));
	const isLast = $derived(nextStep(steps, step) === null);
	const duration = $derived(ui.reducedMotion ? 0 : 160);

	onMount(() => {
		dialog?.showModal();
		if (isTauri()) {
			monitorList()
				.then((list) => (monitors = list))
				.catch(() => (monitors = []));
		}
	});

	// Move focus to the new step's heading so screen readers announce it and Tab starts from the top.
	let firstStep = true;
	$effect(() => {
		void step;
		if (firstStep) {
			firstStep = false;
			return;
		}
		void tick().then(() => heading?.focus({ preventScroll: true }));
	});

	function next() {
		const n = nextStep(steps, step);
		if (n) current = n;
		else finish();
	}

	function back() {
		const p = prevStep(steps, step);
		if (p) current = p;
	}

	function finish() {
		if (finished) return;
		finished = true;
		// A failed write keeps the overlay closed for this session; it comes back on the next start.
		const saved = store.updateSettings(completionPatch());
		saved.catch(() => {});
		ondone(saved);
		if (dialog?.open) dialog.close();
	}

	function oncancel(e: Event) {
		// Esc: skip without a confirmation (skip still counts as completed).
		e.preventDefault();
		finish();
	}

	// ── monitor ──────────────────────────────────────────────────────────────

	const currentMonitor = $derived.by(() => {
		const list = monitors ?? [];
		const name = pickedMonitor ?? store.monitor;
		return (
			list.find((m) => m.name === name) ??
			list.find((m) => m.name === store.settings.monitor) ??
			list.find((m) => !m.primary) ??
			list[0]
		)?.name;
	});

	async function pickMonitor(name: string) {
		if (name === currentMonitor) return;
		const before = pickedMonitor;
		pickedMonitor = name;
		monitorError = null;
		try {
			await monitorSelect(name);
		} catch (e) {
			pickedMonitor = before;
			monitorError = t('onboarding.monitor.failed', { message: e instanceof Error ? e.message : String(e) });
		}
	}

	// ── theme ────────────────────────────────────────────────────────────────

	const themeId = $derived(getTheme(store.settings.theme ?? DEFAULT_THEME_ID).id);
	let filter = $state<'all' | 'dark' | 'light'>(getTheme(store.settings.theme ?? DEFAULT_THEME_ID).variant);
	const FILTERS = ['all', 'dark', 'light'] as const;
	let filterGroup = $state<HTMLElement>();
	/** Radio group keyboard model (same as settings' Segmented): one Tab stop, arrows move the selection. */
	function onFilterKey(e: KeyboardEvent) {
		const dir = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
		if (!dir) return;
		e.preventDefault();
		filter = FILTERS[(FILTERS.indexOf(filter) + dir + FILTERS.length) % FILTERS.length];
		requestAnimationFrame(() => filterGroup?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
	}
	const themeList = $derived([...themes.values()].filter((th) => filter === 'all' || th.variant === filter));

	function pickTheme(id: string) {
		store.setTheme('global', id);
	}

	// ── workspace ────────────────────────────────────────────────────────────

	const choices = $derived(workspaceChoices(store.workspaces));

	function pickWorkspace(id: string) {
		store.switchTo(id);
	}

	const tileTone = (widget: string) => iconTone(definition(widget)?.manifest.icon ?? 'puzzle');
	// "{hotkey} shows or hides…" with the shortcut rendered as a <kbd>, whatever the word order.
	const hotkeyParts = $derived(t('onboarding.done.tipHotkey', { hotkey: '\u0000' }).split('\u0000'));
	const describe = (ws: Workspace) => (isPresetId(ws.id) ? t(`onboarding.workspace.${ws.id}`) : '');
</script>

<dialog bind:this={dialog} class="onboarding" aria-labelledby="k-ob-title" {oncancel}>
	<div class="panel">
		<header class="top">
			<span class="brand"><Mark /><span>{t('shell.appName')}</span></span>
			<span class="count k-mono" aria-hidden="true">{index + 1} / {steps.length}</span>
		</header>

		{#key step}
			<section class="body" in:fade={{ duration }}>
				{#if step === 'welcome'}
					<div class="welcome">
						<span class="hero-mark" aria-hidden="true"><Mark /></span>
						<h1 id="k-ob-title" tabindex="-1" bind:this={heading}>{t('onboarding.welcome.title')}</h1>
						<p class="lead">{t('onboarding.welcome.text')}</p>
					</div>
				{:else if step === 'monitor'}
					<h1 id="k-ob-title" tabindex="-1" bind:this={heading}>{t('onboarding.monitor.title')}</h1>
					<p class="lead">{t('onboarding.monitor.text')}</p>
					<fieldset class="options monitors">
						<legend class="k-sr-only">{t('onboarding.monitor.title')}</legend>
						{#each monitors ?? [] as m (m.name)}
							<label class="option monitor" class:on={m.name === currentMonitor}>
								<input class="k-sr-only" type="radio" name="k-ob-monitor" value={m.name} checked={m.name === currentMonitor} onchange={() => void pickMonitor(m.name)} />
								<span class="tile" aria-hidden="true"><Icon name="monitor" size={20} /></span>
								<span class="text">
									<span class="name">{m.name}</span>
									<span class="meta"><span class="k-mono">{m.width}×{m.height}</span>{#if m.primary} · {t('onboarding.monitor.primary')}{/if}</span>
								</span>
								{#if m.name === currentMonitor}<span class="badge">{t('onboarding.monitor.current')}</span>{/if}
							</label>
						{/each}
					</fieldset>
					{#if monitorError}<p class="error" role="alert">{monitorError}</p>{/if}
				{:else if step === 'theme'}
					<div class="row-head">
						<div>
							<h1 id="k-ob-title" tabindex="-1" bind:this={heading}>{t('onboarding.theme.title')}</h1>
							<p class="lead">{t('onboarding.theme.text')}</p>
						</div>
						<div class="seg" role="radiogroup" aria-label={t('onboarding.theme.filter')} bind:this={filterGroup}>
							{#each FILTERS as f (f)}
								<button
									type="button"
									role="radio"
									aria-checked={filter === f}
									tabindex={filter === f ? 0 : -1}
									onkeydown={onFilterKey}
									onclick={() => (filter = f)}
								>
									{t(`onboarding.theme.${f}`)}
								</button>
							{/each}
						</div>
					</div>
					<fieldset class="themes">
						<legend class="k-sr-only">{t('onboarding.theme.title')}</legend>
						{#each themeList as th (th.id)}
							<label class="option theme" class:on={th.id === themeId}>
								<input class="k-sr-only" type="radio" name="k-ob-theme" value={th.id} checked={th.id === themeId} onchange={() => pickTheme(th.id)} />
								<!-- Swatch colours are theme data, not styling (same as the theme picker). -->
								<span class="swatch" aria-hidden="true">
									<span style:background={th.colors.bg}></span>
									<span style:background={th.colors.surface}></span>
									<span style:background={th.colors.accent}></span>
									<span style:background={th.colors['accent-2']}></span>
								</span>
								<span class="text">
									<span class="name">{th.name}</span>
									<span class="meta">{t(`shell.theme.${th.variant}`)}</span>
								</span>
								{#if th.id === themeId}<span class="check" aria-hidden="true"><Icon name="check" size={16} /></span>{/if}
							</label>
						{/each}
					</fieldset>
				{:else if step === 'workspace'}
					<h1 id="k-ob-title" tabindex="-1" bind:this={heading}>{t('onboarding.workspace.title')}</h1>
					<p class="lead">{t('onboarding.workspace.text')}</p>
					<fieldset class="workspaces">
						<legend class="k-sr-only">{t('onboarding.workspace.title')}</legend>
						{#each choices as ws (ws.id)}
							<label class="option ws" class:on={ws.id === store.activeId}>
								<input class="k-sr-only" type="radio" name="k-ob-workspace" value={ws.id} checked={ws.id === store.activeId} onchange={() => pickWorkspace(ws.id)} />
								<span class="mini" aria-hidden="true" style:grid-template-columns="repeat({ws.grid.cols}, minmax(0, 1fr))" style:grid-template-rows="repeat({ws.grid.rows}, minmax(0, 1fr))">
									{#each ws.widgets as w (w.instance)}
										<span class="cell" style:grid-column="{w.x + 1} / span {w.w}" style:grid-row="{w.y + 1} / span {w.h}" style:--tone={tileTone(w.widget)}></span>
									{/each}
								</span>
								<span class="name">{ws.name}</span>
								{#if describe(ws)}<span class="meta">{describe(ws)}</span>{/if}
							</label>
						{/each}
					</fieldset>
				{:else}
					<h1 id="k-ob-title" tabindex="-1" bind:this={heading}>{t('onboarding.done.title')}</h1>
					<p class="lead">{t('onboarding.done.text')}</p>
					<ul class="tips">
						<li>
							<span class="tile" aria-hidden="true"><Icon name="edit" size={20} /></span>
							<span>{t('onboarding.done.tipEdit')}</span>
						</li>
						<li>
							<span class="tile" aria-hidden="true"><Icon name="keyboard" size={20} /></span>
							<span>{hotkeyParts[0]}<kbd class="k-mono">{store.settings.hotkey}</kbd>{hotkeyParts[1] ?? ''}</span>
						</li>
						<li>
							<span class="tile" aria-hidden="true"><Icon name="gear" size={20} /></span>
							<span>{t('onboarding.done.tipSettings')}</span>
						</li>
					</ul>
				{/if}
			</section>
		{/key}

		<footer class="bottom">
			<div class="left">
				<button type="button" class="k-btn ghost" onclick={finish}>{t('onboarding.skip')}</button>
			</div>
			<ol class="dots" aria-label={t('onboarding.progress', { n: index + 1, total: steps.length })}>
				{#each steps as s, i (s)}
					<li class:on={i === index} class:past={i < index} aria-current={i === index ? 'step' : undefined}>
						<span class="k-sr-only">{t(`onboarding.steps.${s}`)}</span>
					</li>
				{/each}
			</ol>
			<div class="right">
				{#if index > 0}
					<button type="button" class="k-btn" onclick={back}><Icon name="arrowLeft" size={16} />{t('onboarding.back')}</button>
				{/if}
				<!-- svelte-ignore a11y_autofocus -->
				<button type="button" class="k-btn k-btn-primary" autofocus onclick={next}>
					{#if step === 'welcome'}
						{t('onboarding.welcome.start')}
					{:else if isLast}
						{t('onboarding.done.open')}
					{:else}
						{t('onboarding.next')}
					{/if}
					{#if !isLast}<Icon name="arrowRight" size={16} />{/if}
				</button>
			</div>
		</footer>
	</div>
</dialog>

<style>
	.onboarding {
		box-sizing: border-box;
		width: 100vw;
		height: 100vh;
		height: 100dvh;
		max-width: none;
		max-height: none;
		margin: 0;
		padding: 0;
		border: 0;
		/* The deck stays faintly visible behind, so theme and workspace picks show live. */
		background: color-mix(in srgb, var(--k-bg) 95%, transparent);
		color: var(--k-text);
		font-family: var(--k-font-ui);
	}
	.onboarding::backdrop {
		background: transparent;
	}
	.panel {
		box-sizing: border-box;
		height: 100%;
		max-width: 960px;
		margin: 0 auto;
		padding: 16px 32px 20px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.top {
		height: 44px;
		flex: none;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.brand {
		display: inline-flex;
		align-items: center;
		gap: 10px;
		font-size: 14px;
		font-weight: 600;
	}
	.count {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.body {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	h1 {
		margin: 0;
		font-size: 24px;
		font-weight: 600;
		letter-spacing: -0.01em;
	}
	h1:focus {
		outline: none;
	}
	.lead {
		margin: 0 0 8px;
		font-size: 14px;
		color: var(--k-text-muted);
	}
	.welcome {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 12px;
		text-align: center;
	}
	.welcome h1 {
		font-size: 32px;
	}
	.hero-mark :global(svg) {
		width: 56px;
		height: 56px;
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		min-width: 0;
	}

	/* Shared selectable option: a real <label> around a visually hidden radio. */
	.option {
		box-sizing: border-box;
		position: relative;
		display: flex;
		align-items: center;
		gap: 12px;
		min-height: 44px;
		padding: 10px 14px;
		border: 1px solid var(--k-border);
		border-radius: 12px;
		background: var(--k-surface);
		cursor: pointer;
		transition: border-color 150ms ease;
	}
	.option:hover {
		border-color: color-mix(in srgb, var(--k-accent) 50%, var(--k-border));
	}
	.option.on {
		border: 2px solid var(--k-accent);
		padding: 9px 13px;
	}
	.option:has(input:focus-visible) {
		outline: 2px solid var(--k-accent);
		outline-offset: 2px;
	}
	.text {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
		flex: 1;
	}
	.name {
		font-size: 13px;
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.badge {
		flex: none;
		padding: 2px 8px;
		border-radius: 999px;
		font-size: 11px;
		font-weight: 600;
		color: var(--k-text);
		background: color-mix(in srgb, var(--k-accent) 18%, var(--k-surface));
	}
	.tile {
		flex: none;
		display: inline-grid;
		place-items: center;
		width: 40px;
		height: 40px;
		border-radius: 10px;
		background: var(--k-bg);
		color: var(--k-accent);
	}
	.error {
		margin: 4px 0 0;
		font-size: 13px;
		color: var(--k-danger);
	}

	/* Monitor */
	.monitors {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 12px;
	}
	.monitor {
		min-height: 64px;
	}

	/* Theme */
	.row-head {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
	}
	.seg {
		flex: none;
		display: inline-flex;
		gap: 4px;
		padding: 4px;
		border-radius: 999px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
	}
	/* 44 px hit targets (DESIGN §1.3). */
	.seg button {
		min-height: 44px;
		padding: 0 14px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: var(--k-text-muted);
		font-size: 13px;
		font-weight: 500;
	}
	.seg button[aria-checked='true'] {
		background: var(--k-accent);
		color: var(--k-on-accent);
		font-weight: 600;
	}
	.themes {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		/* Room for the focus ring and the 2 px selected border inside the scroller. */
		padding: 4px;
		margin: -4px;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(196px, 1fr));
		grid-auto-rows: min-content;
		gap: 10px;
	}
	.theme {
		padding: 8px 12px 8px 8px;
	}
	.theme.on {
		padding: 7px 11px 7px 7px;
	}
	.swatch {
		flex: none;
		display: inline-flex;
		overflow: hidden;
		border-radius: 8px;
		border: 1px solid var(--k-border);
	}
	.swatch span {
		width: 10px;
		height: 32px;
	}
	.check {
		flex: none;
		display: inline-flex;
		color: var(--k-accent);
	}

	/* Workspace */
	.workspaces {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
		gap: 12px;
	}
	.ws {
		flex-direction: column;
		align-items: stretch;
		gap: 6px;
		padding: 12px;
	}
	.ws.on {
		padding: 11px;
	}
	.mini {
		display: grid;
		gap: 3px;
		aspect-ratio: 16 / 9;
		padding: 6px;
		margin-bottom: 4px;
		border-radius: 8px;
		background: var(--k-bg);
	}
	.cell {
		border-radius: 4px;
		border: 1px solid color-mix(in srgb, var(--tone) 55%, var(--k-border));
		background: color-mix(in srgb, var(--tone) 22%, var(--k-surface));
	}

	/* Done */
	.tips {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
		max-width: 620px;
	}
	.tips li {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 10px 14px;
		border: 1px solid var(--k-border);
		border-radius: 12px;
		background: var(--k-surface);
		font-size: 14px;
	}
	kbd {
		padding: 2px 6px;
		border: 1px solid var(--k-border);
		border-radius: 6px;
		background: var(--k-bg-alt);
		font-size: 12px;
	}

	/* Footer */
	.bottom {
		flex: none;
		display: grid;
		grid-template-columns: 1fr auto 1fr;
		align-items: center;
		gap: 16px;
		padding-top: 12px;
		border-top: 1px solid var(--k-border);
	}
	.right {
		display: flex;
		justify-content: flex-end;
		gap: 10px;
	}
	.ghost {
		border-color: transparent;
		color: var(--k-text-muted);
	}
	.ghost:hover {
		color: var(--k-text);
	}
	.dots {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.dots li {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--k-border);
		transition:
			width 200ms ease,
			background-color 200ms ease;
	}
	.dots li.past {
		background: color-mix(in srgb, var(--k-accent) 45%, var(--k-border));
	}
	.dots li.on {
		width: 22px;
		background: var(--k-accent);
	}

	@media (max-height: 640px) {
		.panel {
			padding: 10px 24px 14px;
			gap: 8px;
		}
		h1 {
			font-size: 20px;
		}
		.welcome h1 {
			font-size: 28px;
		}
		.lead {
			margin-bottom: 4px;
		}
		.mini {
			aspect-ratio: 2 / 1;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.option,
		.dots li {
			transition: none;
		}
	}
</style>
