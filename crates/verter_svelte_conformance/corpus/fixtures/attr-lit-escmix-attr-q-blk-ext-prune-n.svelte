<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="a&b&c">x</div>
{/if}

<style>
	[data-x="\7a \z"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
