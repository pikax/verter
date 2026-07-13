<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} id={flag ? 'b' : '\u0062'}>x</svelte:element>

<style>
	#b {
		color: red;
	}
</style>
