<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div id=a&amp;b&#38;c>x</div>
</div>
{/if}

<style>
	.wrap [data-x="\7a \z"] {
		color: red;
	}
</style>
