{#snippet subject()}
	<div class=a&#32;b>x</div>
{/snippet}

{@render subject()}

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
