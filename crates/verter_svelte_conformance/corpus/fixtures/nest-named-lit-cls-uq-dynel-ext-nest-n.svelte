<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&copy;b>x</svelte:element>

<style>
	.zz {
		& {
			color: red;
		}
	}
</style>
