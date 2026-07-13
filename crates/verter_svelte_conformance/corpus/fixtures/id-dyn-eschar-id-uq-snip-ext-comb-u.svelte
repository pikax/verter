<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div id={value}>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap #a\&b {
		color: red;
	}
</style>
