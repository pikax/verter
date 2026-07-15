<script>
	import Child from './Child.svelte';
	let { x } = $props();
	let v = $state('');
</script>

<Child a="s" b={x} bind:value={v} onclick={() => x}>body {x}</Child>
