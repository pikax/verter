<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&#x26;b">x</div>
{/if}

<style>
	.a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
