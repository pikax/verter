<script lang="ts">
  import { h } from "vue";
  import Greeting from "./Greeting.vue";
  const vnode = h(Greeting, { msg: 1 });
</script>

<p>{String(vnode)}</p>
