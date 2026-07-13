<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
