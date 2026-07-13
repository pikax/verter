<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} id=&#98;>x</svelte:element>
</div>

<style>
	.wrap * {
		color: red;
	}
</style>
