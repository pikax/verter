<script>
  // @ts-check
  /** @type {number} */
  let jsCount = 1;
  /** @type {{ name: string }} */
  let jsUser = { name: "ada" };

  /** @param {number} n */
  function jsBump(n) {
    return n + 1;
  }

  let jsNext = jsBump(jsCount);
</script>

<div>
  <p>{jsCount} {jsUser.name} {jsNext}</p>
  <button onclick={() => (jsCount = jsBump(jsCount))}>{jsCount}</button>
</div>
