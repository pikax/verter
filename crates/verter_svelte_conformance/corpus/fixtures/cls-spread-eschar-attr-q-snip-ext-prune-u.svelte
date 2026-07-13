<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	.a\&b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
