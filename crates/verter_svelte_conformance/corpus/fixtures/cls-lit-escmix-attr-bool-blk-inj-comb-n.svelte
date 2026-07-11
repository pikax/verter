<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div data-x>x</div>
</div>
{/if}

<style>
	.wrap .\7a \z {
		color: red;
	}
</style>
