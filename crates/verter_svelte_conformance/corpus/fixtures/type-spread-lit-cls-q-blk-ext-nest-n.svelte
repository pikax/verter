<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
