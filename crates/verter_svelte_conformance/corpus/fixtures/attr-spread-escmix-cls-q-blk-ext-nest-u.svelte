<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	[data-x="a\26 b\&c"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
