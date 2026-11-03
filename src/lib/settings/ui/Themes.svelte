<script lang="ts">
	/**
	 * Themes: 160 px swatch cards (bundled + user themes), light/dark badge, pair link, and the light/dark
	 * auto switch (off / follow OS / schedule). Swatch colours come from theme data (inline styles).
	 */
	import { t } from '#lib/i18n/index.ts';
	import { DEFAULT_THEME_ID, getTheme, isUserTheme, parseHm, themes, userThemeIssues, type Theme } from '#lib/theme/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Segmented from './Segmented.svelte';

	const { store, env, save } = getSettingsContext();
	const selectedId = $derived(getTheme(store.settings.theme ?? DEFAULT_THEME_ID).id);
	const selected = $derived(getTheme(selectedId));
	const auto = $derived(store.settings.themeAuto);
	const list = $derived([...themes.values()]);
	const issues = $derived([...userThemeIssues.entries()]);
	let grid = $state<HTMLElement>();

	function pick(theme: Theme) {
		void save({ theme: theme.id });
	}

	function showPair(id: string) {
		const el = grid?.querySelector<HTMLElement>(`[data-theme-id="${CSS.escape(id)}"]`);
		el?.scrollIntoView({ block: 'nearest', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
		el?.focus({ preventScroll: true });
	}

	function setTime(key: 'lightFrom' | 'darkFrom', v: string) {
		if (parseHm(v) === null) return;
		void save({ themeAuto: { ...auto, [key]: v } });
	}

	function onRadioKey(e: KeyboardEvent, i: number) {
		const cols = Math.max(1, Math.round((grid?.clientWidth ?? 640) / 172));
		const step: Record<string, number> = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: cols, ArrowUp: -cols };
		if (!(e.key in step)) return;
		e.preventDefault();
		const next = list[Math.min(list.length - 1, Math.max(0, i + step[e.key]))];
		if (next) {
			pick(next);
			showPair(next.id);
		}
	}
</script>

<section class="s-page">
	<Head title={t('settings.themes.title')} text={t('settings.themes.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.themes.auto')}</h2>
		<div class="s-card s-stack">
			<div class="s-row">
				<div class="s-label">
					<span class="s-title">{t('settings.themes.autoLabel')}</span>
					<span class="s-help" class:warn={auto.mode !== 'off' && !selected.pair}>
						{auto.mode !== 'off' && !selected.pair
							? t('settings.themes.noPair', { name: selected.name })
							: t('settings.themes.autoHelp')}
					</span>
				</div>
				<Segmented
					label={t('settings.themes.autoLabel')}
					value={auto.mode}
					options={[
						{ value: 'off', label: t('settings.common.off') },
						{ value: 'os', label: t('settings.themes.os') },
						{ value: 'schedule', label: t('settings.themes.schedule') }
					]}
					onchange={(mode) => void save({ themeAuto: { ...auto, mode } })}
				/>
			</div>
			{#if auto.mode === 'schedule'}
				<div class="s-inline times">
					<label class="s-inline">
						<span class="s-help">{t('settings.themes.lightFrom')}</span>
						<input type="time" class="k-input s-time" value={auto.lightFrom} onchange={(e) => setTime('lightFrom', e.currentTarget.value)} />
					</label>
					<label class="s-inline">
						<span class="s-help">{t('settings.themes.darkFrom')}</span>
						<input type="time" class="k-input s-time" value={auto.darkFrom} onchange={(e) => setTime('darkFrom', e.currentTarget.value)} />
					</label>
				</div>
			{/if}
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow" id="k-theme-grid">{t('settings.themes.global', { count: list.length })}</h2>
		<p class="s-note small">{t('settings.themes.scopeNote')}</p>
		<div class="grid" role="radiogroup" aria-labelledby="k-theme-grid" bind:this={grid}>
			{#each list as theme, i (theme.id)}
				{@const pair = theme.pair ? themes.get(theme.pair) : undefined}
				{@const on = theme.id === selectedId}
				<div class="theme" class:on>
					<button
						type="button"
						role="radio"
						class="pick"
						data-theme-id={theme.id}
						aria-checked={on}
						tabindex={on ? 0 : -1}
						onclick={() => pick(theme)}
						onkeydown={(e) => onRadioKey(e, i)}
					>
						<span class="swatch" aria-hidden="true" style:background={theme.colors.bg} style:border-color={theme.colors.border}>
							<span class="surface" style:background={theme.colors.surface} style:border-color={theme.colors.border}>
								<span class="line" style:background={theme.colors.text}></span>
								<span class="line short" style:background={theme.colors['text-muted']}></span>
							</span>
							<span class="accent" style:background={theme.colors.accent}></span>
							<span class="accent2" style:background={theme.colors['accent-2']}></span>
						</span>
						<span class="name">{theme.name}</span>
						<span class="badges">
							<span class="s-badge">{t(`shell.theme.${theme.variant}`)}</span>
							{#if isUserTheme(theme.id)}<span class="s-badge">{t('shell.theme.user')}</span>{/if}
							{#if on}<Icon name="check" size={16} />{/if}
						</span>
					</button>
					{#if pair}
						<button type="button" class="pair" onclick={() => showPair(pair.id)} aria-label={t('settings.themes.pairLabel', { name: pair.name })}>
							<Icon name="link" size={14} /><span>{pair.name}</span>
						</button>
					{:else}
						<span class="pair none">{t('settings.themes.noPairShort')}</span>
					{/if}
				</div>
			{/each}
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.themes.user')}</h2>
		<p class="s-note small">{t('settings.themes.userHelp')} <span class="s-path">{env.themesPath}</span></p>
		{#if issues.length}
			<ul class="s-list" aria-label={t('settings.themes.skipped')}>
				{#each issues as [label, message] (label)}
					<li class="s-card issue">
						<Icon name="alert" size={16} />
						<span class="s-label">
							<span class="s-title">{label}</span>
							<span class="s-help">{message}</span>
						</span>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</section>

<style>
	.small {
		font-size: 12px;
	}
	.times {
		gap: 20px;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
		gap: 12px;
	}
	.theme {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: var(--k-surface);
		overflow: hidden;
		min-width: 0;
	}
	.theme.on {
		border: 2px solid var(--k-accent);
	}
	.pick {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 10px 10px 6px;
		border: 0;
		background: transparent;
		color: var(--k-text);
		text-align: left;
		border-radius: 10px 10px 0 0;
	}
	.pick:focus-visible {
		outline-offset: -2px;
	}
	.swatch {
		position: relative;
		box-sizing: border-box;
		display: block;
		width: 100%;
		height: 56px;
		border-radius: 8px;
		border: 1px solid;
		overflow: hidden;
	}
	.surface {
		position: absolute;
		left: 8px;
		top: 8px;
		bottom: 8px;
		width: 58%;
		box-sizing: border-box;
		border-radius: 6px;
		border: 1px solid;
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: 5px;
		padding: 0 8px;
	}
	.line {
		display: block;
		height: 4px;
		width: 80%;
		border-radius: 2px;
	}
	.line.short {
		width: 50%;
	}
	.accent {
		position: absolute;
		right: 10px;
		top: 12px;
		width: 30px;
		height: 10px;
		border-radius: 5px;
	}
	.accent2 {
		position: absolute;
		right: 10px;
		bottom: 12px;
		width: 14px;
		height: 14px;
		border-radius: 50%;
	}
	.name {
		font-size: 13px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.badges {
		display: flex;
		align-items: center;
		gap: 6px;
		color: var(--k-accent);
	}
	.pair {
		box-sizing: border-box;
		min-height: 44px;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 10px;
		border: 0;
		border-top: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
		font-size: 12px;
		text-align: left;
		min-width: 0;
	}
	.pair span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		text-decoration: underline;
		text-underline-offset: 3px;
	}
	button.pair:hover {
		color: var(--k-text);
	}
	.pair.none {
		font-size: 11px;
	}
	.issue {
		display: flex;
		align-items: center;
		gap: 10px;
		color: color-mix(in srgb, var(--k-warning) 45%, var(--k-text));
	}
	.issue .s-title {
		color: var(--k-text);
	}
</style>
