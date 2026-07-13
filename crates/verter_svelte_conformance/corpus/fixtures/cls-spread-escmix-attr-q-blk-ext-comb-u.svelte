<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div class="wrap">
	<div {...rest}>x</div>
</div>
{/if}

<style>
	.wrap .a\26 b\&c {
		color: red;
	}
</style>
