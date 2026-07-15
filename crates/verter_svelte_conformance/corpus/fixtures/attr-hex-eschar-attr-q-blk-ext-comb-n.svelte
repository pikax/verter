<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div data-x="a&#x26;b">x</div>
</div>
{/if}

<style>
	.wrap [data-x="\zz"] {
		color: red;
	}
</style>
