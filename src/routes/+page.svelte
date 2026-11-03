<script lang="ts">
	// Phase 0: empty deck shell. Top bar + grid land in Phase 1.
</script>

<main class="flex h-screen flex-col" style="opacity: var(--k-dim)">
	<header class="flex h-14 items-center gap-2.5 px-6">
		<svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
			<path
				d="M12 2 22 12 12 22 2 12Z"
				stroke="var(--k-accent)"
				stroke-width="2"
				stroke-linejoin="round"
			/>
			<path d="M12 7v10M7 12h10" stroke="var(--k-accent-2)" stroke-width="2" stroke-linecap="round" />
		</svg>
		<span class="text-sm font-semibold tracking-[0.01em]">Kotilo Deck</span>
	</header>
	<section class="flex-1" aria-label="Widget grid"></section>
</main>
