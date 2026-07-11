<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
