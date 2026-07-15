<script>
	let el = $state(null);
</script>
<div bind:this={el}></div>
<button onclick={() => console.log(el)}>x</button>
