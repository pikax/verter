<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} class="{value}">x</svelte:element>

<style>
	div {
		color: red;
	}
</style>
