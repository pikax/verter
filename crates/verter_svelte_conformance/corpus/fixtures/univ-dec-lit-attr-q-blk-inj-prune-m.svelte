<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="&#98;">x</div>
{/if}

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
