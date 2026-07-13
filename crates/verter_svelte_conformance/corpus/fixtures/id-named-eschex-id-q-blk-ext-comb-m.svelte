<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div id="a&amp;b">x</div>
</div>
{/if}

<style>
	.wrap #a\26 b {
		color: red;
	}
</style>
