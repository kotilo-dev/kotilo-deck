<script lang="ts" generics="T extends string">
	/** Radio group rendered as a segmented control; 44 px tall options; arrow keys move the selection. */
	let {
		value,
		options,
		label,
		disabled = false,
		onchange
	}: {
		value: T;
		options: readonly { value: T; label: string; mono?: boolean }[];
		label: string;
		disabled?: boolean;
		onchange: (next: T) => void;
	} = $props();

	let root = $state<HTMLElement>();

	function onkeydown(e: KeyboardEvent) {
		const dir = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
		if (!dir) return;
		e.preventDefault();
		const i = options.findIndex((o) => o.value === value);
		const next = options[(i + dir + options.length) % options.length];
		onchange(next.value);
		requestAnimationFrame(() => root?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
	}
</script>

<div bind:this={root} class="s-seg" role="radiogroup" aria-label={label}>
	{#each options as o (o.value)}
		<button
			type="button"
			role="radio"
			aria-checked={o.value === value}
			tabindex={o.value === value ? 0 : -1}
			class:k-mono={o.mono}
			{disabled}
			{onkeydown}
			onclick={() => onchange(o.value)}
		>
			{o.label}
		</button>
	{/each}
</div>
