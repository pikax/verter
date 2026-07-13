<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id=&#98;&#x62;>x</svelte:element>

<style>
	:global(&) {
		color: red;
	}
</style>
