<script lang="ts">
  import { errorText } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../store.svelte";
  import type { SetupContext } from "./context.svelte";

  let { ctx }: { ctx: SetupContext } = $props();

  const probe = $derived(ctx.probe);
  let busy = $state(false);
  let failed = $state<string | null>(null);

  async function run() {
    busy = true;
    failed = null;
    try {
      ctx.imported = await app.importOfficial();
    } catch (e) {
      failed = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="su-stack">
  {#if probe}
    <div class="su-block">
      <h3>{t("setup.import.foundTitle")}</h3>
      <ul class="su-list su-panel">
        <li>
          <span class="name">{t("setup.import.favorites")}</span>
          <b>{probe.favorites}</b>
        </li>
        <li>
          <span class="name">{t("setup.import.hubs")}</span>
          <b>{probe.hubs}</b>
        </li>
        {#if probe.compat_mode !== null}
          <li>
            <span class="name">{t("opt.compat")}</span>
            <b>{probe.compat_mode ? t("setup.on") : t("setup.off")}</b>
          </li>
        {/if}
      </ul>
    </div>
  {/if}

  <p class="su-note">{t("setup.import.safe")}</p>

  {#if ctx.imported}
    {@const done = ctx.imported}
    <p class="su-note good" role="status">
      {t("setup.import.done", { favorites: done.favorites, hubs: done.hubs })}
      {#if done.skipped > 0}{t("setup.import.skipped", { count: done.skipped })}{/if}
    </p>
  {:else}
    <div class="su-actions">
      <button class="btn primary" onclick={run} disabled={busy}>
        {busy ? t("setup.import.busy") : t("setup.import.run")}
      </button>
      <span class="sub">{t("setup.import.optional")}</span>
    </div>
  {/if}
  {#if failed}
    <p class="su-note warn" role="alert">{t("setup.import.failed", { error: failed })}</p>
  {/if}
</div>
