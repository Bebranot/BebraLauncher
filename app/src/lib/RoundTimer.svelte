<script lang="ts">
  import type { ServerRow } from "./api";
  import { roundText } from "./format";
  import { clock } from "./store.svelte";

  let { server }: { server: ServerRow } = $props();

  const running = $derived(server.run_level === 1);

  // Only a round that is actually running needs a clock. Idle rows (lobby, ended, offline)
  // render once and never wake up again, so a 2000-row list stays quiet in idle. The tick
  // is the one shared `clock`, not an interval per row.
  $effect(() => {
    if (!running) return;
    return clock.retain();
  });

  const text = $derived(running ? roundText(server, clock.now) : roundText(server));
</script>

<span>{text ?? ""}</span>
