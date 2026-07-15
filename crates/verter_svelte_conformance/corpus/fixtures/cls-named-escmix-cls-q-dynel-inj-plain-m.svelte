<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a a&amp;b&amp;c">x</svelte:element>

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
