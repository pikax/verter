<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
