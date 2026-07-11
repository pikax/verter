<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a&#32;b">x</div>
{/if}

<style>
	#zz {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
