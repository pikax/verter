<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div id="&#x62;">x</div>
{/if}

<style>
	* {
		color: red;
	}
</style>
