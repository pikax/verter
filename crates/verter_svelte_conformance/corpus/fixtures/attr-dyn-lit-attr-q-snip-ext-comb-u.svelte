<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div data-x="{value}">x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap [data-x="b"] {
		color: red;
	}
</style>
