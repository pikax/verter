<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} data-x="{value}">x</svelte:element>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
