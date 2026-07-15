<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} id=&#x62;>x</svelte:element>
</div>

<style>
	.wrap div {
		color: red;
	}
</style>
