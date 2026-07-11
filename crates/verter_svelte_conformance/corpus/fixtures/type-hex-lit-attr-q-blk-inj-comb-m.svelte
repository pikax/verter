<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div data-x="&#x62;">x</div>
</div>
{/if}

<style>
	.wrap div {
		color: red;
	}
</style>
