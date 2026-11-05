<script lang="ts">
	/**
	 * "Closed unexpectedly last time" notice (SPEC §8 Phase 5). Non-blocking, under the top bar; stays
	 * until the user acts. Report issue and Dismiss both mark the crash as seen; Open logs keeps it.
	 */
	import { crashDismiss, openLogsFolder, openReportIssue, type CrashReport } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import Icon from './Icon.svelte';
	import { getDeck } from './deck.svelte';

	let { crash, onclose }: { crash: CrashReport; onclose: () => void } = $props();

	const { ui } = getDeck();
	let busy = $state(false);

	const fail = (e: unknown) => ui.say(t('crash.failed', { message: e instanceof Error ? e.message : String(e) }), 'warning', 6000);

	async function dismiss() {
		onclose();
		try {
			await crashDismiss();
		} catch {
			// Shows again next start at worst.
		}
	}

	async function report() {
		busy = true;
		try {
			await openReportIssue(crash.summary || null);
			await dismiss();
		} catch (e) {
			fail(e);
		} finally {
			busy = false;
		}
	}

	async function logs() {
		try {
			await openLogsFolder();
		} catch (e) {
			fail(e);
		}
	}
</script>

<div class="banner" role="status" aria-labelledby="k-crash-title">
	<span class="icon" aria-hidden="true"><Icon name="alert" size={18} /></span>
	<div class="text">
		<span class="title" id="k-crash-title">{t('crash.title')}</span>
		{#if crash.summary}<span class="summary k-mono" title={crash.summary}>{crash.summary}</span>{/if}
	</div>
	<div class="actions">
		<button type="button" class="k-btn k-btn-primary" disabled={busy} onclick={report}>
			<Icon name="bug" size={16} />{t('crash.report')}
		</button>
		<button type="button" class="k-btn" onclick={logs}>
			<Icon name="folder" size={16} />{t('crash.logs')}
		</button>
		<button type="button" class="k-btn k-btn-icon" aria-label={t('crash.dismiss')} title={t('crash.dismiss')} onclick={dismiss}>
			<Icon name="x" size={18} />
		</button>
	</div>
</div>

<style>
	.banner {
		position: fixed;
		top: 64px;
		left: 50%;
		z-index: 30;
		transform: translateX(-50%);
		box-sizing: border-box;
		width: max-content;
		max-width: min(760px, calc(100vw - 48px));
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px 8px 8px 16px;
		border-radius: 14px;
		border: 1px solid var(--k-warning);
		background: color-mix(in srgb, var(--k-warning) 14%, var(--k-bg-alt));
		color: var(--k-text);
		font-family: var(--k-font-ui);
		animation: k-crash-in 200ms ease-out;
	}
	.icon {
		flex: none;
		display: inline-flex;
		color: var(--k-warning);
	}
	.text {
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
	}
	.summary {
		font-size: 11px;
		color: var(--k-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.actions {
		flex: none;
		display: flex;
		gap: 6px;
	}
	@keyframes k-crash-in {
		from {
			opacity: 0;
			transform: translate(-50%, -6px);
		}
	}
	@media (max-width: 900px) {
		.banner {
			flex-wrap: wrap;
		}
		.text {
			flex: 1 1 60%;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.banner {
			animation: none;
		}
	}
</style>
