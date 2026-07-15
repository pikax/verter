<script>
	let on = $state(false);
	let off = $state(false);
	let c = $state('a');
</script>

<button onclick={() => on = !on} class="base" class:foo={on}></button>
<button onclick={() => off = !off} class={c} class:foo={on} class:bar={!off}></button>
