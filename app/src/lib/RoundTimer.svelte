<script lang="ts">
  import type { ServerRow } from "./api";
  import { roundText } from "./format";

  let { server }: { server: ServerRow } = $props();

  const running = $derived(server.run_level === 1);
  let now = $state(Date.now());

  // Only a round that is actually running needs a clock. Idle rows (lobby, ended, offline)
  // render once and never wake up again, so a 2000-row list stays quiet in idle.
  $effect(() => {
    if (!running) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const text = $derived(running ? roundText(server, now) : roundText(server));
</script>

<span>{text ?? ""}</span>
