<script lang="ts">
	/**
	 * Settings window route (Tauri window label "settings"); also opens directly in a browser. A LAN
	 * tablet/browser (remote mode) gets the deck only: settings stay on the host computer.
	 */
	import { isRemote } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import SettingsApp from '#lib/settings/ui/SettingsApp.svelte';

	const remote = isRemote();
</script>

<svelte:head><title>Kotilo Deck — Settings</title></svelte:head>

{#if remote}
	<main class="remote">
		<section class="card" aria-labelledby="k-remote-title">
			<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="var(--k-accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
				<rect x="2" y="4" width="20" height="13" rx="2" />
				<path d="M8 21h8M12 17v4" />
			</svg>
			<h1 id="k-remote-title">{t('remote.settingsTitle')}</h1>
			<p>{t('remote.settingsText')}</p>
			<a class="k-btn k-btn-primary" href="/">{t('remote.backToDeck')}</a>
		</section>
	</main>
{:else}
	<SettingsApp />
{/if}

<style>
	.remote {
		box-sizing: border-box;
		min-height: 100vh;
		min-height: 100dvh;
		display: grid;
		place-items: center;
		padding: 24px;
		background: var(--k-bg);
		color: var(--k-text);
		font-family: var(--k-font-ui);
	}
	.card {
		box-sizing: border-box;
		max-width: 420px;
		padding: 28px;
		border-radius: var(--k-radius);
		border: 1px solid var(--k-border);
		background: var(--k-surface);
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 12px;
		text-align: center;
	}
	h1 {
		margin: 0;
		font-size: 20px;
		font-weight: 600;
	}
	p {
		margin: 0 0 8px;
		font-size: 13px;
		color: var(--k-text-muted);
	}
	a {
		text-decoration: none;
	}
</style>
