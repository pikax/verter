<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div class=a&copy;b>x</div>
</div>
{/if}

<style>
	.wrap * {
		color: red;
	}
</style>
