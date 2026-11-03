<script lang="ts">
	import '../app.css';
	import { applyTheme, getTheme, DEFAULT_THEME_ID } from '#lib/theme/index.ts';

	let { children } = $props();

	applyTheme(getTheme(DEFAULT_THEME_ID));
</script>

{@render children()}
