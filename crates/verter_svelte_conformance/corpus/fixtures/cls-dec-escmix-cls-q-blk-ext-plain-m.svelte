<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&#38;b&#38;c">x</div>
{/if}

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
