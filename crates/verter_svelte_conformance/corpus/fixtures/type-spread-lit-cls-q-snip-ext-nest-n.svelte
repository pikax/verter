<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
