<script>
	let x = $state(false);
	let c = $state('red');
</script>

<div class:a={x} style:color={c}></div>
<button onclick={() => { x = !x; c = 'blue'; }}>go</button>
