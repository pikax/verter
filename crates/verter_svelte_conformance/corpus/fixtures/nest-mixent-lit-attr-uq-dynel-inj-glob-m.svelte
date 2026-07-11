<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x=&#98;&#x62;>x</svelte:element>

<style>
	:global(&) {
		color: red;
	}
</style>
