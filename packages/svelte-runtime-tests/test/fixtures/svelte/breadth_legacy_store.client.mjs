import "svelte/internal/disclose-version";
import "svelte/internal/flags/legacy";
import * as $ from "svelte/internal/client";
import { writable } from "svelte/store";

var root = $.from_html(`<button> </button>`);

export default function App($$anchor, $$props) {
  $.push($$props, false);
  const $count = () => $.store_get(count, "$count", $$stores);
  const [$$stores, $$cleanup] = $.setup_stores();
  const count = writable(1);
  let doubled = $.mutable_source();
  $.legacy_pre_effect(
    () => $count(),
    () => {
      $.set(doubled, $count() * 2);
    },
  );
  $.legacy_pre_effect_reset();
  $.init();
  var button = root();
  var text = $.child(button, true);
  $.reset(button);
  $.template_effect(() => $.set_text(text, $.get(doubled)));
  $.event("click", button, () => $.update_store(count, $count()));
  $.append($$anchor, button);
  $.pop();
  $$cleanup();
}
