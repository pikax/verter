<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&amp;b">x</div>
{/if}

<style>
	.a\26 b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
