<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&#x26;b>x</svelte:element>

<style>
	.a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
