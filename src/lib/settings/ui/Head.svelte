<script lang="ts">
	/** Section heading: H1 20/600 + one-line description 13 muted (DESIGN §7). Focus target on navigation. */
	let { title, text }: { title: string; text: string } = $props();
</script>

<header class="s-head">
	<h1 tabindex="-1">{title}</h1>
	<p>{text}</p>
</header>
