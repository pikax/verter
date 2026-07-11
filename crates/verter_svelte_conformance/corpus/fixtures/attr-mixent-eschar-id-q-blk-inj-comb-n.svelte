<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div id="a&#38;&#x62;">x</div>
</div>
{/if}

<style>
	.wrap [data-x="\zz"] {
		color: red;
	}
</style>
