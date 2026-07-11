<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x>x</div>
{/if}

<style>
	[data-zz] {
		& {
			color: red;
		}
	}
</style>
