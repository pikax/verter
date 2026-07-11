<svelte:options css="injected" />

<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<svelte:element this={tag} {...rest}>x</svelte:element>

<style>
	#a\26 b\&c {
		color: red;
	}
</style>
