{#snippet subject()}
	<div id="a&amp;b">x</div>
{/snippet}

{@render subject()}

<style>
	#a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
