<script lang="ts">
	import type { Family } from './logic';

	interface Props {
		family: Family;
		isDay?: boolean;
		size: number;
		stroke?: number;
	}
	let { family, isDay = true, size, stroke = 1.6 }: Props = $props();

	// Cloud raised to leave room for precipitation (DESIGN reference 2×1 rain icon).
	const highCloud = 'M8 15h9a3.5 3.5 0 0 0 .3-7 5 5 0 0 0-9.6-1.3A4 4 0 0 0 8 15Z';
</script>

<svg
	class="wx"
	width={size}
	height={size}
	viewBox="0 0 24 24"
	fill="none"
	stroke-width={stroke}
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
>
	{#if family === 'clear'}
		{#if isDay}
			<g class="sun">
				<circle cx="12" cy="12" r="4" />
				<path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" />
			</g>
		{:else}
			<path class="cloud" d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z" />
		{/if}
	{:else if family === 'partly'}
		{#if isDay}
			<g class="sun">
				<circle cx="9" cy="9" r="3.5" />
				<path d="M9 2v1.5M2 9h1.5M14.5 9H16M4 4l1 1M13 4l-1 1" />
			</g>
		{:else}
			<path class="cloud" d="M13 8.5A5 5 0 1 1 7.5 3a4 4 0 0 0 5.5 5.5Z" />
		{/if}
		<path class="cloud" d="M11 20h7.5a3 3 0 0 0 .2-6 4.5 4.5 0 0 0-8.6-1.2A3.6 3.6 0 0 0 11 20Z" />
	{:else if family === 'cloud'}
		<path class="cloud" d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
	{:else if family === 'fog'}
		<path class="cloud" d={highCloud} />
		<path class="cloud" d="M5 18.5h14M7 21.5h10" />
	{:else if family === 'drizzle'}
		<path class="cloud" d={highCloud} />
		<path class="drop" d="M8 18.5v.5M12 18.5v.5M16 18.5v.5M10 21.5v.5M14 21.5v.5" />
	{:else if family === 'rain'}
		<path class="cloud" d={highCloud} />
		<path class="drop" d="M8 18v2M12 18v3M16 18v2" />
	{:else if family === 'snow'}
		<path class="cloud" d={highCloud} />
		<path class="flake" d="M8 18.5v.5M12 18v.5M16 18.5v.5M10 21.5v.5M14 21.5v.5" />
	{:else}
		<path class="cloud" d={highCloud} />
		<path class="bolt" d="m13 15-3 4h4l-3 4" />
	{/if}
</svg>

<style>
	.wx {
		flex-shrink: 0;
	}
	.sun,
	.bolt {
		stroke: var(--k-warning);
	}
	.cloud {
		stroke: var(--k-text-muted);
	}
	.drop {
		stroke: var(--k-accent);
	}
	.flake {
		stroke: var(--k-text);
	}
</style>
