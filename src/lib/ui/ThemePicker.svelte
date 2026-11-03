<script lang="ts">
	/** Theme chip (DESIGN §4.2) + popover. Scope: global setting or the active workspace's override. */
	import { t } from '#lib/i18n/index.ts';
	import { effectiveThemeId, getTheme, themeLabel, themes } from '#lib/theme/index.ts';
	import Icon from './Icon.svelte';
	import Popover from './Popover.svelte';
	import { getDeck } from './deck.svelte';

	const { store } = getDeck();
	const ws = $derived(store.active);
	const currentId = $derived(effectiveThemeId(ws?.theme, store.settings.theme));
	const current = $derived(getTheme(currentId));

	let scope = $state<'global' | 'workspace'>('global');
	function onOpen() {
		scope = ws?.theme ? 'workspace' : 'global';
	}

	function pick(id: string) {
		store.setTheme(scope, id);
	}

	const selectedId = $derived(scope === 'workspace' ? (ws?.theme ?? null) : getTheme(store.settings.theme).id);
</script>

<button
	type="button"
	class="chip-btn"
	popovertarget="k-theme-picker"
	aria-label={t('shell.topbar.themeChip', { name: current.name })}
>
	<span class="chip">{themeLabel(current)}</span>
</button>

<Popover id="k-theme-picker" label={t('shell.theme.title')} align="end" width={300} onopen={onOpen}>
	<div class="wrap">
		<div class="seg" role="radiogroup" aria-label={t('shell.theme.title')}>
			<button type="button" role="radio" aria-checked={scope === 'global'} onclick={() => (scope = 'global')}>
				{t('shell.theme.scopeGlobal')}
			</button>
			<button type="button" role="radio" aria-checked={scope === 'workspace'} onclick={() => (scope = 'workspace')}>
				{t('shell.theme.scopeWorkspace', { name: ws?.name ?? '' })}
			</button>
		</div>
		<ul role="listbox" aria-label={t('shell.theme.title')}>
			{#if scope === 'workspace'}
				<li>
					<button
						type="button"
						class="k-menu-item"
						role="option"
						aria-selected={selectedId === null}
						onclick={() => store.setTheme('workspace', null)}
					>
						<span class="check">{#if selectedId === null}<Icon name="check" size={16} />{/if}</span>
						<span class="name">{t('shell.theme.useGlobal')}</span>
					</button>
				</li>
			{/if}
			{#each themes.values() as theme (theme.id)}
				<li>
					<button
						type="button"
						class="k-menu-item"
						role="option"
						aria-selected={selectedId === theme.id}
						onclick={() => pick(theme.id)}
					>
						<span class="check">{#if selectedId === theme.id}<Icon name="check" size={16} />{/if}</span>
						<span class="swatch" aria-hidden="true">
							<span style:background={theme.colors.bg}></span>
							<span style:background={theme.colors.surface}></span>
							<span style:background={theme.colors.accent}></span>
							<span style:background={theme.colors['accent-2']}></span>
						</span>
						<span class="name">{theme.name}</span>
						<span class="variant">{t(`shell.theme.${theme.variant}`)}</span>
					</button>
				</li>
			{/each}
		</ul>
	</div>
</Popover>

<style>
	.chip-btn {
		display: inline-flex;
		align-items: center;
		height: 44px;
		padding: 0 2px;
		border: 0;
		background: transparent;
	}
	.chip {
		padding: 4px 10px;
		border: 1px solid var(--k-border);
		border-radius: 8px;
		font-family: var(--k-font-mono);
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.chip-btn:hover .chip {
		color: var(--k-text);
	}
	/* Ring the visible chip, not the larger 44 px hit box. */
	.chip-btn:focus-visible {
		outline: none;
	}
	.chip-btn:focus-visible .chip {
		outline: 2px solid var(--k-accent);
		outline-offset: 2px;
	}
	.wrap {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.seg {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 4px;
		padding: 4px;
		border-radius: 12px;
		background: var(--k-bg);
		border: 1px solid var(--k-border);
	}
	.seg button {
		min-height: 36px;
		padding: 0 8px;
		border: 0;
		border-radius: 9px;
		background: transparent;
		color: var(--k-text-muted);
		font-size: 12px;
		font-weight: 500;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.seg button[aria-checked='true'] {
		background: var(--k-surface);
		color: var(--k-text);
		font-weight: 600;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.check {
		display: inline-flex;
		width: 16px;
		color: var(--k-accent);
	}
	.swatch {
		display: inline-flex;
		overflow: hidden;
		border-radius: 6px;
		border: 1px solid var(--k-border);
	}
	.swatch span {
		width: 9px;
		height: 20px;
	}
	.name {
		flex: 1;
	}
	.variant {
		font-size: 11px;
		color: var(--k-text-muted);
	}
</style>
