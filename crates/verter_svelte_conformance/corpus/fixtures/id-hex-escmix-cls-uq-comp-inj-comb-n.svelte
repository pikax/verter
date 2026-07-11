<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div class=a&#x26;b&#x26;c>x</div>
</div>
</Child>

<style>
	.wrap #\7a \z {
		color: red;
	}
</style>
