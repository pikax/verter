<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x=&#x62;>x</svelte:element>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
