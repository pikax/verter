<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div class={value}>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap .a\26 b {
		color: red;
	}
</style>
