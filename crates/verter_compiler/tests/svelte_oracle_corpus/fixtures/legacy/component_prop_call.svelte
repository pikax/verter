<script>
	import Child from './Child.svelte';
	export let obj;
</script>

<Child foo={obj.m()} />
