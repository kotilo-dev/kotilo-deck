<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import {
		companionOpenDesktop,
		companionRespond,
		companionSnapshot,
		companionStatus,
		hasBackend,
		on,
		type Agent,
		type Approval,
		type CompanionSnapshot,
		type CompanionStatus
	} from '#lib/ipc/index.ts';
	import { isDeckVisible, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		agentLabel,
		formatCost,
		formatTokens,
		isConnected,
		minutesIn,
		pendingApprovals,
		runningCount,
		settleHidden,
		statusTone,
		usageBar,
		viewOf
	} from './logic';
	import { defaults, type AgentsSettings } from './settings';

	let { size, settings, editing, available }: WidgetProps<AgentsSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const tauri = hasBackend();

	/** Agent row height and gap; approval card height (8+8 padding, 2 border, 16+14 lines, 2+2 gaps, 6 + 36 px buttons) and gap. */
	const ROW = 28;
	const ROW_GAP = 2;
	const APPR = 94;
	const APPR_GAP = 8;

	let status = $state<CompanionStatus | null>(null);
	let snapshot = $state<CompanionSnapshot | null>(null);
	let failed = $state(false);
	let retryKey = $state(0);
	let visible = $state(isDeckVisible());
	let now = $state(Date.now());

	/** Approvals answered here whose `companionRespond` is in flight or already resolved upstream. */
	let hidden = $state<Set<string>>(new Set());
	let respondError = $state(false);
	let errorTimer: ReturnType<typeof setTimeout> | null = null;
	let openFailed = $state(false);

	function receive(next: CompanionSnapshot) {
		snapshot = next;
		hidden = settleHidden(hidden, Array.isArray(next.approvals) ? next.approvals : []);
	}

	$effect(() => onDeckVisibility((v) => (visible = v)));

	// Push-only events; listen while shown, re-read status + snapshot whenever the deck is shown again.
	$effect(() => {
		void retryKey;
		if (!tauri || !available || !visible) return;
		let disposed = false;
		const offs: (() => void)[] = [];
		const keep = (p: Promise<() => void>) =>
			p.then((u) => (disposed ? u() : offs.push(u))).catch(() => {});
		keep(on('companion://status', (st) => (status = st)));
		keep(on('companion://snapshot', receive));
		companionStatus()
			.then((st) => {
				if (disposed) return;
				status = st;
				failed = false;
			})
			.catch(() => {
				if (!disposed && !status) failed = true;
			});
		companionSnapshot()
			.then((snap) => {
				if (!disposed) receive(snap);
			})
			.catch(() => {});
		return () => {
			disposed = true;
			for (const off of offs) off();
		};
	});

	$effect(() => () => {
		if (errorTimer) clearTimeout(errorTimer);
	});

	const connected = $derived(tauri ? isConnected(status) : false);
	const view = $derived(viewOf(connected ? snapshot : null));
	const approvals = $derived(pendingApprovals(view.approvals, hidden));
	const agentsById = $derived(new Map(view.agents.map((a) => [a.id, a])));
	const running = $derived(runningCount(view.agents));
	const shownAgents = $derived(s.showIdle ? view.agents : view.agents.filter((a) => a.status !== 'idle'));

	// "12 min" in the rows: minute tick while the list is on screen.
	$effect(() => {
		if (!connected || !visible || size === '2x1') return;
		now = Date.now();
		return subscribeTick((d) => (now = d.getTime()));
	});

	async function respond(approval: Approval, decision: 'approve' | 'deny') {
		if (editing) return;
		hidden = new Set([...hidden, approval.id]);
		try {
			await companionRespond(approval.id, decision);
		} catch {
			const next = new Set(hidden);
			next.delete(approval.id);
			hidden = next;
			respondError = true;
			if (errorTimer) clearTimeout(errorTimer);
			errorTimer = setTimeout(() => (respondError = false), 4000);
		}
	}

	async function openDesktop() {
		if (editing) return;
		openFailed = false;
		try {
			await companionOpenDesktop();
		} catch {
			openFailed = true;
		}
	}

	// ── Text ───────────────────────────────────────────────────────────────
	const headline = $derived(
		view.agents.length === 0
			? t('widgets.agents.none')
			: running === 0
				? t(view.agents.length === 1 ? 'widgets.agents.idleOne' : 'widgets.agents.idleMany', { n: view.agents.length })
				: t(running === 1 ? 'widgets.agents.runningOne' : 'widgets.agents.runningMany', { n: running })
	);
	const costShort = $derived(view.usage ? formatCost(view.usage.costUsd, locale) : '');
	const cost = $derived(view.usage ? t('widgets.agents.today', { cost: costShort }) : '');
	const bar = $derived(usageBar(view.usage));

	function statusLine(agent: Agent): string {
		if (agent.status === 'waiting') return t('widgets.agents.waitingTask', { task: agent.task || t('widgets.agents.approval') });
		if (agent.status === 'working') return agent.task ? t('widgets.agents.workingTask', { task: agent.task }) : t('widgets.agents.working');
		return t('widgets.agents.idle');
	}

	const whoOf = (a: Approval) => {
		const agent = agentsById.get(a.agentId);
		return agent ? agentLabel(agent) : '';
	};

	const minutesText = (agent: Agent) => {
		const m = minutesIn(agent.since, now);
		return m === null || agent.status === 'idle' ? '' : t('widgets.agents.minutes', { n: m });
	};

	// ── Fitting (cards never scroll) ───────────────────────────────────────
	let listH = $state(0);
	let queueH = $state(0);
	const fit = (h: number, row: number, gap: number) => (h <= 0 ? 0 : Math.max(0, Math.floor((h + gap) / (row + gap))));
	const approvalCap = $derived(size === '2x2' ? 1 : Math.max(1, fit(queueH, APPR, APPR_GAP)));
	const queue = $derived(approvals.slice(0, approvalCap));
	const moreApprovals = $derived(approvals.length - queue.length);
	const agentCap = $derived(fit(listH, ROW, ROW_GAP));
	const agentRows = $derived(shownAgents.slice(0, agentCap));
	const moreAgents = $derived(shownAgents.length - agentRows.length);
	const top = $derived(approvals[0] ?? null);
	const topAgent = $derived(top ? (agentsById.get(top.agentId) ?? null) : (shownAgents[0] ?? view.agents[0] ?? null));
</script>

{#snippet checkIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>
{/snippet}
{#snippet xIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
{/snippet}
{#snippet offlineIcon()}
	<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" /><path d="M8 20h8M12 16v4" /><path d="m9.5 7.5 5 5M14.5 7.5l-5 5" />
	</svg>
{/snippet}

{#snippet actions(a: Approval, wide: boolean)}
	{@const who = whoOf(a)}
	<div class="actions" class:wide data-no-gesture>
		<button
			type="button"
			class="act approve"
			aria-label={t('widgets.agents.approveLabel', { summary: a.summary })}
			title={a.detail ?? a.summary}
			disabled={editing}
			onclick={() => respond(a, 'approve')}
		>
			{@render checkIcon()}<span class="act-label">{t('widgets.agents.approve')}</span>
		</button>
		<button
			type="button"
			class="act deny"
			aria-label={t('widgets.agents.denyLabel', { summary: a.summary })}
			title={who ? `${who}: ${a.summary}` : a.summary}
			disabled={editing}
			onclick={() => respond(a, 'deny')}
		>
			{@render xIcon()}<span class="act-label">{t('widgets.agents.deny')}</span>
		</button>
	</div>
{/snippet}

{#snippet header()}
	<header class="hdr">
		<span class="lead">
			<span class="dot" style:background={connected ? 'var(--k-success)' : 'var(--k-border)'} aria-hidden="true"></span>
			<span class="title">{connected ? headline : t('widgets.agents.title')}</span>
		</span>
		{#if connected}
			<span class="meta" class:warn-text={respondError} aria-live="polite">
				{#if respondError}{t('widgets.agents.respondFailed')}{:else if view.usage}<span class="mono cost-long">{cost}</span><span class="mono cost-short">{costShort}</span>{/if}
			</span>
		{:else if status && !status.enabled}
			<span class="meta">{t('widgets.agents.off')}</span>
		{/if}
	</header>
{/snippet}

{#snippet agentList()}
	<ul class="alist" bind:clientHeight={listH} aria-label={t('widgets.agents.list')}>
		{#each agentRows as agent (agent.id)}
			<li class="agent">
				<span class="dot" style:background="var(--k-{statusTone(agent.status)})" aria-hidden="true"></span>
				<span class="a-text">
					<span class="a-name">{agentLabel(agent)}</span>
					<span class="a-status" class:warn-text={agent.status === 'waiting'}>{statusLine(agent)}</span>
				</span>
				<span class="a-time mono">{minutesText(agent)}</span>
			</li>
		{/each}
		{#if shownAgents.length === 0}
			<li class="agent empty">{t('widgets.agents.noAgents')}</li>
		{/if}
	</ul>
	{#if moreAgents > 0}<span class="more">{t('widgets.agents.moreAgents', { n: moreAgents })}</span>{/if}
{/snippet}

{#snippet approvalQueue()}
	<ul class="queue" aria-label={t('widgets.agents.queue')}>
		{#each queue as a (a.id)}
			{@const who = whoOf(a)}
			<li class="appr">
				<span class="appr-sum" title={a.detail ?? a.summary}>{a.summary}</span>
				{#if who}<span class="appr-who">{who}</span>{/if}
				{@render actions(a, true)}
			</li>
		{/each}
	</ul>
	{#if moreApprovals > 0}<span class="more warn-text">{t('widgets.agents.moreApprovals', { n: moreApprovals })}</span>{/if}
{/snippet}

<section
	class="k-card agents"
	data-size={size}
	data-attention={connected && approvals.length > 0 ? 'true' : undefined}
	aria-label={t('widgets.agents.title')}
>
	{#if tauri && !available}
		<div class="state">
			{@render offlineIcon()}
			<span>{t('widgets.common.notAvailable')}</span>
		</div>
	{:else if tauri && !status && !failed}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar" style="width: 50%"></span>
			<span class="bar"></span>
			{#if size !== '2x1'}<span class="bar"></span>{/if}
		</div>
	{:else if tauri && failed && !status}
		<div class="state">
			<span class="state-title">{t('widgets.agents.error')}</span>
			<button type="button" class="k-btn" disabled={editing} onclick={() => (retryKey += 1)}>{t('widgets.common.retry')}</button>
		</div>
	{:else if !connected}
		<!-- DESIGN §5.10: muted, never an error colour. -->
		{#if size !== '2x1'}{@render header()}{/if}
		<div class="offline" class:row={size === '2x1'}>
			<span class="off-icon">{@render offlineIcon()}</span>
			<span class="off-text" aria-live="polite">{openFailed ? t('widgets.agents.openFailed') : t('widgets.agents.disconnected')}</span>
			<button type="button" class="k-btn ghost-btn" disabled={editing} onclick={openDesktop}>{t('widgets.agents.open')}</button>
		</div>
	{:else if size === '2x1'}
		{@render header()}
		<div class="compact">
			{#if top}
				<span class="c-text">
					<span class="c-name">{topAgent ? agentLabel(topAgent) : top.summary}</span>
					<span class="c-status warn-text" title={top.detail ?? top.summary}>
						{t('widgets.agents.waitingTask', { task: top.summary })}{#if approvals.length > 1}<span class="mono"> +{approvals.length - 1}</span>{/if}
					</span>
				</span>
				{@render actions(top, false)}
			{:else if topAgent}
				<span class="c-text">
					<span class="c-name">{agentLabel(topAgent)}</span>
					<span class="c-status">{statusLine(topAgent)}</span>
				</span>
			{:else}
				<span class="c-status">{t('widgets.agents.noAgents')}</span>
			{/if}
		</div>
	{:else if size === '2x2'}
		{@render header()}
		{#if queue.length}
			<div class="queue-wrap fixed">{@render approvalQueue()}</div>
		{/if}
		<div class="list-wrap">{@render agentList()}</div>
	{:else}
		{@render header()}
		<div class="wide">
			<div class="list-wrap">{@render agentList()}</div>
			<div class="queue-wrap" bind:clientHeight={queueH}>
				{#if queue.length}
					{@render approvalQueue()}
				{:else}
					<span class="no-appr">{t('widgets.agents.noApprovals')}</span>
				{/if}
			</div>
		</div>
		{#if view.usage}
			{@const u = view.usage}
			<div class="usage">
				<div class="u-row">
					<span class="tiny">{t('widgets.agents.usage')}</span>
					<span class="mono u-val">
						{t('widgets.agents.tokens', { n: formatTokens(u.tokens, locale) })}
						{#if bar.fraction === null}
							· {formatCost(u.costUsd, locale)}
						{/if}
					</span>
					{#if bar.fraction !== null && u.budgetUsd !== null}
						<span class="mono u-val u-cost" class:warn-text={bar.warn}>
							{formatCost(u.costUsd, locale)} / {formatCost(u.budgetUsd, locale)}
						</span>
					{/if}
				</div>
				{#if bar.fraction !== null}
					<div
						class="track"
						role="meter"
						aria-label={t('widgets.agents.budget')}
						aria-valuemin={0}
						aria-valuemax={100}
						aria-valuenow={Math.round(bar.fraction * 100)}
					>
						<div class="fill" class:warn={bar.warn} style:width="{bar.fraction * 100}%"></div>
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</section>

<style>
	.agents {
		--warn-text: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
		container-type: size;
		padding: 18px 20px;
		gap: 10px;
		font-family: var(--k-font-ui);
	}
	.agents[data-size='2x1'] {
		padding: 14px 18px;
		gap: 8px;
		justify-content: space-between;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.warn-text {
		color: var(--warn-text);
	}
	.hdr {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 10px;
		min-width: 0;
		flex-shrink: 0;
	}
	.lead {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		flex-shrink: 0;
	}
	.meta.warn-text {
		color: var(--warn-text);
		flex-shrink: 1;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		flex-shrink: 0;
	}

	/* Approve (filled success) / Deny (ghost): 36 px secondary in-card buttons (DESIGN §5.10). */
	.actions {
		display: flex;
		gap: 6px;
		flex-shrink: 0;
	}
	.actions.wide .act {
		flex: 1;
	}
	.act {
		box-sizing: border-box;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		height: 36px;
		min-width: 36px;
		padding: 0 12px;
		border-radius: 10px;
		font-size: 12px;
		font-weight: 600;
		white-space: nowrap;
	}
	.act svg {
		display: none;
	}
	.approve {
		border: 0;
		/* Mixed toward text so on-accent labels keep 4.5:1 on light themes too (Latte success is 3.2:1 on white). */
		background: color-mix(in srgb, var(--k-success) 60%, var(--k-text));
		color: var(--k-on-accent);
	}
	.approve:hover:not(:disabled) {
		background: color-mix(in srgb, var(--k-success) 50%, var(--k-text));
	}
	.deny {
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
		font-weight: 500;
	}
	.deny:hover:not(:disabled) {
		background: color-mix(in srgb, var(--k-text) 6%, transparent);
	}

	/* 2×1 */
	.compact {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		min-width: 0;
	}
	.c-text {
		display: flex;
		flex-direction: column;
		gap: 1px;
		min-width: 0;
	}
	.c-name {
		font-size: 12px;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.c-status {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.c-status.warn-text {
		color: var(--warn-text);
	}

	/* Lists */
	.list-wrap,
	.queue-wrap {
		flex: 1;
		min-height: 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
		overflow: hidden;
	}
	.queue-wrap.fixed {
		flex: none;
	}
	.agents ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.alist {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		overflow: hidden;
	}
	.agent {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex-shrink: 0;
		min-width: 0;
	}
	.agent.empty {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.a-text {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: baseline;
		gap: 6px;
		white-space: nowrap;
		overflow: hidden;
	}
	.a-name {
		font-size: 12px;
		font-weight: 500;
		flex-shrink: 0;
		max-width: 60%;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.a-status {
		font-size: 11px;
		color: var(--k-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.a-status.warn-text {
		color: var(--warn-text);
	}
	.a-time {
		font-size: 11px;
		color: var(--k-text-muted);
		flex-shrink: 0;
	}
	.more,
	.no-appr {
		font-size: 11px;
		color: var(--k-text-muted);
		flex-shrink: 0;
	}
	.more.warn-text {
		color: var(--warn-text);
	}
	.no-appr {
		margin: auto;
	}
	.queue {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
	}
	.appr {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 8px 10px;
		border-radius: 10px;
		background: var(--k-bg-alt);
		border: 1px solid color-mix(in srgb, var(--k-warning) 55%, var(--k-border));
		flex-shrink: 0;
	}
	.appr-sum {
		font-size: 12px;
		line-height: 16px;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.appr-who {
		font-size: 11px;
		line-height: 14px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.appr .actions {
		margin-top: 6px;
	}

	/* 4×2 */
	.wide {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
		gap: 16px;
	}
	.usage {
		display: flex;
		flex-direction: column;
		gap: 6px;
		flex-shrink: 0;
	}
	.u-row {
		display: flex;
		align-items: baseline;
		gap: 10px;
		min-width: 0;
	}
	.tiny {
		font-size: 11px;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		white-space: nowrap;
	}
	.u-val {
		font-size: 12px;
		white-space: nowrap;
	}
	.u-cost {
		margin-left: auto;
	}
	.track {
		height: 6px;
		border-radius: 3px;
		background: var(--k-bg-alt);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		border-radius: 3px;
		background: var(--k-accent);
		transition: width 200ms ease;
	}
	.fill.warn {
		background: var(--k-warning);
	}

	/* Disconnected (muted) */
	.offline {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		text-align: center;
		color: var(--k-text-muted);
		min-height: 0;
	}
	.off-icon {
		display: flex;
	}
	.off-text {
		font-size: 12px;
	}
	.offline.row {
		flex-direction: row;
		justify-content: space-between;
		text-align: left;
		gap: 10px;
	}
	.offline.row .off-text {
		flex: 1;
		min-width: 0;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.ghost-btn {
		flex-shrink: 0;
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		text-align: center;
		font-size: 12px;
		color: var(--k-text-muted);
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
		gap: 10px;
	}
	.bar {
		display: block;
		height: 14px;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}

	.cost-short {
		display: none;
	}

	/* 1024×600 and narrow 2×N: icon-only Approve / Deny (labels stay as aria-label), cost without "today". */
	@container (width < 250px) {
		.cost-long {
			display: none;
		}
		.cost-short {
			display: inline;
		}
		.compact .act {
			padding: 0;
			width: 36px;
		}
		.compact .act svg {
			display: block;
		}
		.compact .act-label {
			display: none;
		}
		.offline.row .off-icon {
			display: none;
		}
	}
	@container (height < 200px) {
		.offline:not(.row) .off-icon {
			display: none;
		}
	}
</style>
