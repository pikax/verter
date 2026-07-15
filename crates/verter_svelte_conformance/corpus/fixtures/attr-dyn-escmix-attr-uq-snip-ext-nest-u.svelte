<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div data-x={value}>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="a\26 b\&c"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
