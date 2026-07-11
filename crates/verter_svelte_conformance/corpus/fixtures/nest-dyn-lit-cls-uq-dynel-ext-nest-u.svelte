<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} class={value}>x</svelte:element>

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
