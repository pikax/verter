<script>
	let id = $state('a');
	let c = $state('b');
	let s = $state('color:red');
</script>

<button onclick={() => { id += '!'; c += '!'; s += '!'; }} id={id} class={c} style={s}></button>
