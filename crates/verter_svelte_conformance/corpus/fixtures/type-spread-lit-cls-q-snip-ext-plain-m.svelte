<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	div {
		color: red;
	}
</style>
