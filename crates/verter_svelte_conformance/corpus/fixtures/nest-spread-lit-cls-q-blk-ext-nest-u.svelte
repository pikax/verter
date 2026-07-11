<script>
	let open = $state(true);
	let { rest } = $props();
</script>

{#if open}
	<div {...rest}>x</div>
{/if}

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
