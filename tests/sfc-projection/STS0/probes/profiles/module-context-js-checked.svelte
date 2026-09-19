<script module>
  // @ts-check
  /** @type {number} */
  export const moduleAnswer = 42;
</script>

<script>
  // @ts-check
  /** @type {number} */
  const shown = moduleAnswer;
</script>

<p>{shown}</p>
