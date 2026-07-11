<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} data-x=&#98;&#x62;>x</svelte:element>
</div>

<style>
	.wrap div {
		color: red;
	}
</style>
