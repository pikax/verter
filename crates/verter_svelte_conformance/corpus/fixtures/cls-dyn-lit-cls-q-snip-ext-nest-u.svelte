<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class="{value}">x</div>
{/snippet}

{@render subject()}

<style>
	.b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
