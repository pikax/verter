<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&amp;b&amp;c">x</div>
{/if}

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
