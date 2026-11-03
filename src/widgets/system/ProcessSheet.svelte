<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import { systemTopProcesses, type ProcessInfo } from '#lib/ipc/index.ts';
	import { memLabel } from './logic';

	/** Refresh cadence while the sheet is open. */
	const REFRESH_MS = 2000;

	let dialog: HTMLDialogElement | undefined = $state();
	let processes = $state<ProcessInfo[] | null>(null);
	let failed = $state(false);
	let timer: ReturnType<typeof setInterval> | null = null;
	let isOpen = false;
	const titleId = `sys-proc-${Math.random().toString(36).slice(2, 8)}`;

	async function refresh() {
		try {
			const list = await systemTopProcesses();
			if (!isOpen) return;
			processes = list.slice(0, 5);
			failed = false;
		} catch {
			if (isOpen) failed = true;
		}
	}

	/** Opens the modal sheet. Native `<dialog>` gives Esc-to-close and makes the rest of the deck inert. */
	export function open() {
		if (!dialog || dialog.open) return;
		isOpen = true;
		processes = null;
		failed = false;
		dialog.showModal();
		refresh();
		timer = setInterval(refresh, REFRESH_MS);
	}

	function onclose() {
		isOpen = false;
		if (timer !== null) clearInterval(timer);
		timer = null;
	}

	function onclick(e: MouseEvent) {
		// Tap on the backdrop (outside the panel) closes.
		if (e.target === dialog) dialog?.close();
	}

	$effect(() => () => {
		onclose();
		if (dialog?.open) dialog.close();
	});
</script>

<dialog bind:this={dialog} class="sheet" aria-labelledby={titleId} {onclose} {onclick}>
	<div class="panel">
		<header class="head">
			<h2 id={titleId}>{t('widgets.system.processes')}</h2>
			<button type="button" class="close" aria-label={t('widgets.common.close')} onclick={() => dialog?.close()}>
				<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
					<path d="M18 6 6 18M6 6l12 12" />
				</svg>
			</button>
		</header>
		{#if failed && !processes}
			<p class="note">{t('widgets.system.processesFailed')}</p>
		{:else if processes === null}
			<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
				{#each [0, 1, 2, 3, 4] as i (i)}<span class="bar"></span>{/each}
			</div>
		{:else if processes.length === 0}
			<p class="note">{t('widgets.system.noProcesses')}</p>
		{:else}
			<table>
				<thead>
					<tr>
						<th scope="col">{t('widgets.system.procName')}</th>
						<th scope="col" class="num">{t('widgets.system.procCpu')}</th>
						<th scope="col" class="num">{t('widgets.system.procMem')}</th>
					</tr>
				</thead>
				<tbody>
					{#each processes as p, i (`${i}-${p.name}`)}
						<tr>
							<td class="name">{p.name}</td>
							<td class="num mono">{Math.round(p.cpu)}%</td>
							<td class="num mono">{memLabel(p.memory, locale)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</dialog>

<style>
	.sheet {
		/* Tailwind preflight zeroes margins; restore the native centring of a modal dialog. */
		margin: auto;
		width: min(440px, calc(100vw - 48px));
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
	.close {
		width: 44px;
		height: 44px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
		cursor: pointer;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		table-layout: fixed;
		font-size: 13px;
	}
	th {
		font-size: 11px;
		font-weight: 400;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		text-align: left;
		padding: 0 0 6px;
	}
	td {
		height: 40px;
		border-top: 1px solid var(--k-border);
	}
	.num {
		text-align: right;
		width: 84px;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.name {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
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
		height: 30px;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}
</style>
