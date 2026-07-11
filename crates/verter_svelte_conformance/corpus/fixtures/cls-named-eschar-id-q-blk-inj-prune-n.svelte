<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div id="a&amp;b">x</div>
{/if}

<style>
	.\zz {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
