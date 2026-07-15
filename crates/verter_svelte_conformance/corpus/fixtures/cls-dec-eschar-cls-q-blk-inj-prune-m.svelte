<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&#38;b">x</div>
{/if}

<style>
	.a\&b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
