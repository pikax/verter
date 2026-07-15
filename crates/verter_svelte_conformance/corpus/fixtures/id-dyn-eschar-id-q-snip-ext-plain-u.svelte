<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div id="{value}">x</div>
{/snippet}

{@render subject()}

<style>
	#a\&b {
		color: red;
	}
</style>
