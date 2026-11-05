<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import { supportedTimeZones } from '#lib/time/index.ts';
	import { searchZones, zoneOffsetLabel } from './logic';

	interface Props {
		/** Zones that cannot be picked again (already in use). */
		exclude?: string[];
		onpick: (timeZone: string) => void;
		oncancel: () => void;
	}
	let { exclude = [], onpick, oncancel }: Props = $props();

	const all = supportedTimeZones();
	let query = $state('');
	const results = $derived(searchZones(all, query, 60).filter((tz) => !exclude.includes(tz)).slice(0, 40));
	const id = `zp-${Math.random().toString(36).slice(2, 8)}`;

	function focus(node: HTMLInputElement) {
		node.focus();
	}
	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.stopPropagation();
			oncancel();
		} else if (e.key === 'Enter' && results[0]) {
			e.preventDefault();
			onpick(results[0]);
		}
	}
</script>

<div class="picker">
	<div class="row">
		<label class="k-sr-only" for={id}>{t('widgets.clock.settings.search')}</label>
		<input
			{id}
			class="field"
			type="search"
			autocomplete="off"
			spellcheck="false"
			placeholder={t('widgets.clock.settings.search')}
			bind:value={query}
			{onkeydown}
			use:focus
		/>
		<button type="button" class="btn ghost" onclick={oncancel}>{t('widgets.clock.settings.cancel')}</button>
	</div>
	{#if results.length}
		<ul class="results" role="list">
			{#each results as tz (tz)}
				<li>
					<button type="button" class="result" onclick={() => onpick(tz)}>
						<span class="name">{tz.replace(/_/g, ' ')}</span>
						<span class="off">{zoneOffsetLabel(tz)}</span>
					</button>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="empty">{t('widgets.clock.settings.noResults')}</p>
	{/if}
</div>

<style>
	.picker {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.row {
		display: flex;
		gap: 8px;
	}
	.field {
		flex: 1;
		min-width: 0;
		height: 44px;
		box-sizing: border-box;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text);
		font: 13px var(--k-font-ui);
	}
	.btn {
		height: 44px;
		padding: 0 14px;
		border-radius: 12px;
		font: 600 13px var(--k-font-ui);
		cursor: pointer;
	}
	.ghost {
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
	}
	.results {
		margin: 0;
		padding: 4px;
		list-style: none;
		max-height: 264px;
		overflow-y: auto;
		border: 1px solid var(--k-border);
		border-radius: 10px;
		background: var(--k-bg-alt);
	}
	.result {
		width: 100%;
		min-height: 44px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 0 10px;
		border: 0;
		border-radius: 8px;
		background: transparent;
		color: var(--k-text);
		font: 13px var(--k-font-ui);
		text-align: left;
		cursor: pointer;
	}
	.result:hover {
		background: var(--k-surface);
	}
	.off {
		font: 11px var(--k-font-mono);
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.empty {
		margin: 0;
		font-size: 12px;
		color: var(--k-text-muted);
	}
</style>
