<script>
	let c = $state(0);
</script>

<button id="x" disabled class="base" type="submit">{c}</button>
<button onclick={() => c++}>bump</button>
