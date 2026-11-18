<script lang="ts">
  import { h } from "vue";
  import Greeting from "./Greeting.vue";
  const vnode = h(Greeting, { msg: "hi" });
</script>

<p>{String(vnode)}</p>
