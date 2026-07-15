<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div {...rest}>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap [data-x="a\26 b"] {
		color: red;
	}
</style>
