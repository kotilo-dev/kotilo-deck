<script lang="ts">
	/** DESIGN §7 switch (44×26, 44 px tall hit area). */
	let {
		checked,
		label,
		describedby,
		disabled = false,
		onchange
	}: { checked: boolean; label: string; describedby?: string; disabled?: boolean; onchange: (next: boolean) => void } = $props();
</script>

<button
	type="button"
	role="switch"
	class="k-switch"
	aria-checked={checked}
	aria-label={label}
	aria-describedby={describedby}
	{disabled}
	onclick={() => onchange(!checked)}
></button>
