<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div id="{value}">x</div>
{/snippet}

{@render subject()}

<style>
	#a\26 b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
