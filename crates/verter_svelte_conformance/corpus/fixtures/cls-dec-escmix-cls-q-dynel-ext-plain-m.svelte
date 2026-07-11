<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a a&#38;b&#38;c">x</svelte:element>

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
