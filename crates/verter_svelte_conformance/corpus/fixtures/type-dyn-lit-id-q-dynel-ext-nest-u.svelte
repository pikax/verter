<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} id="{value}">x</svelte:element>

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
