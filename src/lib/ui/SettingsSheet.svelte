<script lang="ts">
	/**
	 * Per-instance settings sheet (DESIGN §6 gear): the widget's own Settings.svelte (if any) on top,
	 * then a generic form from the manifest schema (boolean / enum / number / string), then Appearance.
	 */
	import { t } from '#lib/i18n/index.ts';
	import type { SettingSchema } from '../../widgets/index';
	import FallbackCard from './FallbackCard.svelte';
	import Icon from './Icon.svelte';
	import { definition, getDeck, mergedSettings, optionLabel, settingLabel, widgetName, surfaceDefault } from './deck.svelte';

	const { store, ui } = getDeck();
	let dialog = $state<HTMLDialogElement>();

	const inst = $derived(ui.settingsFor ? store.active?.widgets.find((w) => w.instance === ui.settingsFor) : undefined);
	const def = $derived(inst ? definition(inst.widget) : undefined);
	const merged = $derived(inst ? mergedSettings(inst) : {});
	const fields = $derived(
		Object.entries(def?.manifest.settings?.properties ?? {}).filter(
			(entry): entry is [string, Exclude<SettingSchema, { type: 'array' | 'object' }>] =>
				entry[1].type === 'boolean' || entry[1].type === 'string' || entry[1].type === 'number'
		)
	);
	const appearance = $derived((inst?.settings?.appearance ?? {}) as { opacity?: number; blur?: number });
	const fallbackOpacity = $derived(surfaceDefault(store));
	const opacityPct = $derived(Math.round((appearance.opacity ?? fallbackOpacity) * 100));

	$effect(() => {
		if (!dialog) return;
		if (inst && !dialog.open) dialog.showModal();
		if (!inst && dialog.open) dialog.close();
	});

	function set(key: string, value: unknown) {
		if (!inst) return;
		store.updateWidgetSettings(inst.instance, { ...(inst.settings ?? {}), [key]: value });
	}

	function setOpacity(pct: number | null) {
		if (!inst) return;
		const next = { ...appearance };
		if (pct === null) delete next.opacity;
		else next.opacity = Math.min(1, Math.max(0, pct / 100));
		const settings = { ...(inst.settings ?? {}) };
		if (Object.keys(next).length) settings.appearance = next;
		else delete settings.appearance;
		store.updateWidgetSettings(inst.instance, settings);
	}

	function remove() {
		if (!inst) return;
		const name = widgetName(inst.widget);
		store.removeWidget(inst.instance);
		ui.settingsFor = null;
		ui.say(t('shell.edit.removed', { name }));
	}
</script>

<dialog
	bind:this={dialog}
	class="k-dialog sheet"
	aria-labelledby="k-sheet-title"
	onclose={() => (ui.settingsFor = null)}
	onclick={(e) => {
		if (e.target === dialog) dialog?.close();
	}}
>
	{#if inst}
		<div class="inner">
			<header>
				<h2 id="k-sheet-title">{t('shell.edit.settings', { name: widgetName(inst.widget) })}</h2>
				<button type="button" class="k-btn k-btn-icon" aria-label={t('shell.sheet.close')} onclick={() => dialog?.close()}>
					<Icon name="x" size={16} stroke={2.4} />
				</button>
			</header>

			<div class="scroll">
				{#if def?.settingsComponent}
					{@const Custom = def.settingsComponent}
					<section class="group">
						<svelte:boundary>
							<Custom settings={merged} onchange={(next) => store.updateWidgetSettings(inst.instance, next)} />
							{#snippet failed(_e, reset)}
								<FallbackCard compact icon="alert" title={widgetName(inst.widget)} body={t('shell.widget.error')} onretry={reset} />
							{/snippet}
						</svelte:boundary>
					</section>
				{/if}

				{#if fields.length}
					<section class="group">
						{#each fields as [key, schema] (key)}
							{@const label = settingLabel(inst.widget, key, schema.title)}
							{@const value = merged[key] ?? schema.default}
							{#if schema.type === 'boolean'}
								<label class="row toggle">
									<span>{label}</span>
									<input type="checkbox" class="k-check" checked={value === true} onchange={(e) => set(key, e.currentTarget.checked)} />
								</label>
							{:else if schema.type === 'string' && schema.enum}
								<label class="row">
									<span>{label}</span>
									<select class="k-select" value={String(value ?? '')} onchange={(e) => set(key, e.currentTarget.value)}>
										{#each schema.enum as opt (opt)}
											<option value={opt}>{optionLabel(inst.widget, key, opt)}</option>
										{/each}
									</select>
								</label>
							{:else if schema.type === 'string'}
								<label class="row">
									<span>{label}</span>
									<input class="k-input" value={String(value ?? '')} onchange={(e) => set(key, e.currentTarget.value)} />
								</label>
							{:else if schema.type === 'number' && schema.minimum !== undefined && schema.maximum !== undefined}
								<label class="row">
									<span class="split">{label}<span class="k-mono val">{value ?? ''}</span></span>
									<input
										type="range"
										class="k-range"
										min={schema.minimum}
										max={schema.maximum}
										step={schema.step ?? 1}
										value={Number(value ?? schema.minimum)}
										oninput={(e) => set(key, Number(e.currentTarget.value))}
									/>
								</label>
							{:else if schema.type === 'number'}
								<label class="row">
									<span>{label}</span>
									<input
										type="number"
										class="k-input k-mono"
										min={schema.minimum}
										max={schema.maximum}
										step={schema.step ?? 'any'}
										value={value ?? ''}
										onchange={(e) => {
											const n = e.currentTarget.valueAsNumber;
											if (Number.isFinite(n)) set(key, n);
										}}
									/>
								</label>
							{/if}
						{/each}
					</section>
				{:else if !def?.settingsComponent}
					<p class="muted">{t('shell.sheet.noSettings')}</p>
				{/if}

				<section class="group">
					<h3>{t('shell.sheet.appearance')}</h3>
					<label class="row">
						<span class="split">
							{t('shell.sheet.opacity')}
							<span class="k-mono val">{opacityPct}%</span>
						</span>
						<input type="range" class="k-range" min="0" max="100" step="1" value={opacityPct} oninput={(e) => setOpacity(Number(e.currentTarget.value))} />
					</label>
					<div class="split">
						<span class="muted small">{appearance.opacity === undefined ? t('shell.sheet.opacityDefault') : ''}</span>
						{#if appearance.opacity !== undefined}
							<button type="button" class="k-btn" onclick={() => setOpacity(null)}>{t('shell.sheet.reset')}</button>
						{/if}
					</div>
				</section>
			</div>

			<footer>
				<button type="button" class="k-btn k-btn-danger" onclick={remove}>
					<Icon name="trash" size={16} />{t('shell.sheet.remove')}
				</button>
			</footer>
		</div>
	{/if}
</dialog>

<style>
	.sheet {
		margin: 0 0 0 auto;
		height: 100%;
		max-height: 100%;
		width: 380px;
		max-width: 100vw;
		border-radius: 16px 0 0 16px;
		border-right: 0;
	}
	/* Light scrim so the opacity slider's live preview on the card stays visible. */
	.sheet::backdrop {
		background: color-mix(in srgb, var(--k-bg) 25%, transparent);
	}
	.inner {
		box-sizing: border-box;
		height: 100%;
		display: flex;
		flex-direction: column;
	}
	header,
	footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 14px 16px 14px 20px;
	}
	header {
		border-bottom: 1px solid var(--k-border);
	}
	footer {
		border-top: 1px solid var(--k-border);
	}
	h2 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}
	h3 {
		margin: 0;
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--k-text-muted);
	}
	.scroll {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 16px 20px;
		display: flex;
		flex-direction: column;
		gap: 16px;
	}
	.group {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 14px;
		border-radius: 12px;
		background: var(--k-surface);
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-size: 13px;
	}
	.row.toggle {
		flex-direction: row;
		align-items: center;
		justify-content: space-between;
		min-height: 44px;
		cursor: pointer;
	}
	.split {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
	}
	.val {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.muted {
		margin: 0;
		color: var(--k-text-muted);
		font-size: 13px;
	}
	.small {
		font-size: 12px;
	}
</style>
