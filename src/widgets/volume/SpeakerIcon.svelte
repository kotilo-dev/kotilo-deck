<script lang="ts">
	/** Speaker glyph (Lucide-style stroke): 0 = slashed (muted/silent), 1 = one wave, 2 = two waves. */
	let { level, size = 18 }: { level: 0 | 1 | 2; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
	<path d="M11 5 6 9H2v6h4l5 4V5Z" />
	{#if level === 0}
		<path d="m22 9-6 6M16 9l6 6" />
	{:else}
		<path d="M15.5 8.5a5 5 0 0 1 0 7" />
		{#if level === 2}<path d="M19 5a10 10 0 0 1 0 14" />{/if}
	{/if}
</svg>
