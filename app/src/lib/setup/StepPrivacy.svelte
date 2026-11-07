<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type HwidState } from "../api";
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";
  import Row from "./Row.svelte";

  const s = $derived(app.settings!);

  let hwid = $state<HwidState | null>(null);

  onMount(() => {
    void api
      .getHwid()
      .then((h) => (hwid = h))
      .catch(() => {});
  });

  async function setAuto(on: boolean) {
    try {
      hwid = await api.setHwidAuto(on);
    } catch (e) {
      app.say(errorText(e));
    }
  }
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("opt.hwid")}</h3>
    <p class="sub">{t("opt.hwidDesc")}</p>
    <div class="su-panel">
      {#if hwid}
        <Option
          label={t("opt.hwidAuto")}
          hint={t("opt.hwidAutoHint")}
          checked={hwid.auto}
          onchange={setAuto}
        />
      {:else}
        <span class="sub">{t("opt.hwidLoading")}</span>
      {/if}
    </div>
    {#if hwid && !hwid.patch}
      <p class="su-note warn">{t("opt.hwidNoPatch")}</p>
    {/if}
    <p class="sub small">{t("setup.privacy.hwidMore")}</p>
  </div>

  <div class="su-block">
    <h3>{t("opt.discordRpc")}</h3>
    <div class="su-panel">
      <Row label={t("setup.privacy.rpcLabel")} hint={t("opt.discordRpcHint")}>
        <select
          value={s.discord_rpc_mode}
          onchange={(e) => app.updateSettings({ discord_rpc_mode: e.currentTarget.value })}
        >
          <option value="vanilla">{t("opt.discordRpcVanilla")}</option>
          <option value="launcher">{t("opt.discordRpcLauncher")}</option>
          <option value="off">{t("opt.discordRpcOff")}</option>
        </select>
      </Row>
    </div>
  </div>
</div>

<style>
  .small {
    margin: 0;
    font-size: 13px;
    line-height: 1.4;
  }
</style>
