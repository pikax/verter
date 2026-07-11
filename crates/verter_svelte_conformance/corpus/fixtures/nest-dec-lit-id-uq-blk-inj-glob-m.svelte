<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div id=&#98;>x</div>
{/if}

<style>
	:global(&) {
		color: red;
	}
</style>
