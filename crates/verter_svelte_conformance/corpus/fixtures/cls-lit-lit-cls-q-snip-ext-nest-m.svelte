{#snippet subject()}
	<div class="a b">x</div>
{/snippet}

{@render subject()}

<style>
	.b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
