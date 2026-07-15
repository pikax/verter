<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	[data-x="a\&b"] {
		color: red;
	}
</style>
