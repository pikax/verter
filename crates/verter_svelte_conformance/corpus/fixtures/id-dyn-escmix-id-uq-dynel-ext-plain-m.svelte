<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} id={flag ? 'a&b&c' : 'a\u0026b\u0026c'}>x</svelte:element>

<style>
	#a\26 b\&c {
		color: red;
	}
</style>
