<svelte:options css="injected" />

<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<svelte:element this={tag} {...rest}>x</svelte:element>

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
