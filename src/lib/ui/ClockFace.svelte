<script lang="ts">
	/**
	 * Screensaver clock face after inactivity (SPEC §2 burn-in): large mono clock that drifts to a new
	 * spot once a minute (1.5 s glide; reduced motion → a jump). Tap or any key wakes.
	 */
	import { t, locale } from '#lib/i18n/index.ts';
	import { dtf } from '#lib/time/index.ts';
	import { clockDrift } from '#lib/settings/look.ts';

	let { now, onwake }: { now: Date; onwake: () => void } = $props();

	const time = $derived(dtf(locale, { hour: '2-digit', minute: '2-digit' }).format(now));
	const date = $derived(dtf(locale, { weekday: 'long', day: 'numeric', month: 'long' }).format(now));
	const drift = $derived(clockDrift(now.getHours() * 60 + now.getMinutes()));
</script>

<button type="button" class="face" data-wake aria-label={t('shell.screensaver.wake')} onclick={onwake}>
	<span class="clock" style:--fx={drift.fx} style:--fy={drift.fy}>
		<span class="time k-mono">{time}</span>
		<span class="date">{date}</span>
	</span>
</button>

<style>
	.face {
		position: absolute;
		inset: 0;
		z-index: 40;
		padding: 0;
		border: 0;
		background: var(--k-bg);
		color: var(--k-text);
		cursor: default;
		container-type: size;
		overflow: hidden;
	}
	.clock {
		position: absolute;
		left: 0;
		top: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		transform: translate(calc(var(--fx) * (100cqw - 100%)), calc(var(--fy) * (100cqh - 100%)));
		/* A short glide once a minute, then the compositor idles (a 60 s glide would render frames all night). */
		transition: transform 1.5s ease;
	}
	.time {
		font-size: clamp(96px, 16cqw, 220px);
		font-weight: 500;
		letter-spacing: -0.04em;
		line-height: 1;
	}
	.date {
		font-size: clamp(16px, 2.2cqw, 28px);
		color: var(--k-text-muted);
	}
	@media (prefers-reduced-motion: reduce) {
		.clock {
			transition: none;
		}
	}
</style>
