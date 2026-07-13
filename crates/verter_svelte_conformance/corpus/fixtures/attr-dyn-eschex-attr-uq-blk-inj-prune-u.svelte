<svelte:options css="injected" />

<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div data-x={value}>x</div>
{/if}

<style>
	[data-x="a\26 b"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
