<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&#x26;b&#x26;c>x</svelte:element>

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
