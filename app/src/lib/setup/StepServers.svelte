<script lang="ts">
  import { onMount } from "svelte";
  import { api, type RecommendedHub } from "../api";
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";
  import Row from "./Row.svelte";
  import { sameHub } from "./steps";

  const s = $derived(app.settings!);

  let recommended = $state<RecommendedHub[]>([]);
  onMount(() => {
    void api
      .getRecommendedHubs()
      .then((r) => (recommended = r))
      .catch(() => {});
  });

  const missing = $derived(recommended.filter((r) => !s.hubs.some((h) => sameHub(h.url, r.url))));

  function nameOf(url: string): string {
    return recommended.find((r) => sameHub(r.url, url))?.name ?? url;
  }

  function toggle(index: number, enabled: boolean) {
    app.updateSettings({ hubs: s.hubs.map((h, i) => (i === index ? { ...h, enabled } : h)) });
    void app.refreshServers();
  }

  function add(hub: RecommendedHub) {
    if (s.hubs.some((h) => sameHub(h.url, hub.url))) return;
    app.updateSettings({ hubs: [...s.hubs, { url: hub.url, enabled: true }] });
    void app.refreshServers();
  }
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("opt.hubs")}</h3>
    <p class="sub">{t("setup.servers.hubsIntro")}</p>
    <div class="su-panel">
      {#each s.hubs as hub, i (hub.url)}
        <Option
          label={nameOf(hub.url)}
          hint={nameOf(hub.url) === hub.url ? "" : hub.url}
          checked={hub.enabled}
          onchange={(v) => toggle(i, v)}
        />
      {:else}
        <span class="sub">{t("setup.servers.noHubs")}</span>
      {/each}
    </div>
    {#if missing.length > 0}
      <div class="su-actions">
        <span class="sub">{t("opt.recommendedHubs")}:</span>
        {#each missing as hub (hub.url)}
          <button class="btn soft" onclick={() => add(hub)}>+ {hub.name}</button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="su-block">
    <h3>{t("setup.servers.listTitle")}</h3>
    <div class="su-panel">
      <Row label={t("opt.autoRefresh")} hint={t("setup.servers.refreshHint")}>
        <select
          value={s.auto_refresh_interval}
          onchange={(e) => app.updateSettings({ auto_refresh_interval: Number(e.currentTarget.value) })}
        >
          <option value={0}>{t("opt.autoRefreshOff")}</option>
          <option value={15}>{t("opt.autoRefresh15")}</option>
          <option value={30}>{t("opt.autoRefresh30")}</option>
          <option value={60}>{t("opt.autoRefresh60")}</option>
          <option value={120}>{t("opt.autoRefresh120")}</option>
        </select>
      </Row>
    </div>
    <p class="su-note">{t("setup.servers.favorites")}</p>
  </div>
</div>
