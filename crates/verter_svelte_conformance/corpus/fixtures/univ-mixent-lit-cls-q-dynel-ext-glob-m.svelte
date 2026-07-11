<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a &#98;&#x62;">x</svelte:element>

<style>
	:global(*) {
		color: red;
	}
</style>
