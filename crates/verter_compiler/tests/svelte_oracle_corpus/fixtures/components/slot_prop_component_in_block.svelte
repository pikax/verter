<script>
	import Inner from './Inner.svelte';
	let { show } = $props();
</script>

{#if show}
	<Inner slot="c" />
{/if}
