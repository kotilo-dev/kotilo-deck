<script lang="ts">
	/**
	 * Light-dismiss popover in the top layer (native `popover="auto"`: Esc and outside click close it).
	 * Positioned under its trigger (`popovertarget={id}`) each time it opens.
	 */
	import type { Snippet } from 'svelte';

	let {
		id,
		label,
		align = 'start',
		width = 260,
		children,
		onopen,
		el = $bindable()
	}: {
		id: string;
		label: string;
		align?: 'start' | 'center' | 'end';
		width?: number;
		children: Snippet;
		onopen?: () => void;
		el?: HTMLDivElement;
	} = $props();

	function place(e: Event) {
		const ev = e as ToggleEvent;
		if (ev.newState !== 'open' || !el) return;
		onopen?.();
		const trigger = document.querySelector<HTMLElement>(`[popovertarget="${id}"]`);
		if (!trigger) return;
		const r = trigger.getBoundingClientRect();
		const vw = window.innerWidth;
		let left = align === 'start' ? r.left : align === 'end' ? r.right - width : r.left + r.width / 2 - width / 2;
		left = Math.max(8, Math.min(vw - width - 8, left));
		el.style.left = `${left}px`;
		el.style.top = `${r.bottom + 8}px`;
		el.style.maxHeight = `${window.innerHeight - r.bottom - 16}px`;
	}
</script>

<div
	bind:this={el}
	{id}
	popover="auto"
	class="k-popover"
	role="dialog"
	aria-label={label}
	style:width="{width}px"
	onbeforetoggle={place}
>
	{@render children()}
</div>

<style>
	div {
		overflow: auto;
	}
</style>
