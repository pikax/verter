<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a b">x</div>
{/if}

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
