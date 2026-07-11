<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div class="wrap">
	<div data-x={value}>x</div>
</div>
{/if}

<style>
	.wrap [data-x="a\&b"] {
		color: red;
	}
</style>
