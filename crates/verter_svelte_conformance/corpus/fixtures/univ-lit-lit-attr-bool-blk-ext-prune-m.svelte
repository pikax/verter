<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x>x</div>
{/if}

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
