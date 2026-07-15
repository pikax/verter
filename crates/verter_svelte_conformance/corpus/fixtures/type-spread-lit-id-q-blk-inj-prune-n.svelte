<svelte:options css="injected" />

<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	p {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
