<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} id="{value}">x</svelte:element>

<style>
	#a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
