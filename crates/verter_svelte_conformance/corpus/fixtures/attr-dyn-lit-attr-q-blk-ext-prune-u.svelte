<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div data-x="{value}">x</div>
{/if}

<style>
	[data-x="b"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
