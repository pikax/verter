{#snippet subject()}
	<div id=a&b&c>x</div>
{/snippet}

{@render subject()}

<style>
	#a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
