<div><slot></slot><p>y</p></div>

<style>
	:global(.x) + p { color: red; }
	.x + p { color: blue; }
</style>
