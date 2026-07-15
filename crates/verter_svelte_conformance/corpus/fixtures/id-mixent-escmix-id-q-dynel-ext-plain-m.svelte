<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&amp;b&#38;c">x</svelte:element>

<style>
	#a\26 b\&c {
		color: red;
	}
</style>
