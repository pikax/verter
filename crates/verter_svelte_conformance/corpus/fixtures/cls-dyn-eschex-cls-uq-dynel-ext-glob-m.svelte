<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} class={flag ? 'a a&b' : 'a\u0026b'}>x</svelte:element>

<style>
	:global(.a\26 b) {
		color: red;
	}
</style>
