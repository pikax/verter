<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} class={value}>x</svelte:element>

<style>
	.a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
