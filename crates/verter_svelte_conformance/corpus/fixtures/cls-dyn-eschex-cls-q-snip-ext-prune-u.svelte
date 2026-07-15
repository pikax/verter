<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class="{value}">x</div>
{/snippet}

{@render subject()}

<style>
	.a\26 b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
