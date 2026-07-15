<svelte:options css="injected" />

<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} data-x={flag ? 'b' : '\u0062'}>x</svelte:element>

<style>
	* {
		color: red;
	}
</style>
