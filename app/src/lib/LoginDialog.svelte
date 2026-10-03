<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorText } from "./api";
  import { t, type Key } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let {
    onclose,
    relogin = undefined,
  }: {
    onclose: () => void;
    /** Log in again to a saved account whose login has expired. */
    relogin?: { userId: string; username: string };
  } = $props();

  // The dialog is made anew each time, so the starting name is read once.
  let username = $state(untrack(() => relogin?.username ?? ""));
  let password = $state("");
  let tfaCode = $state("");
  let askTfa = $state(false);
  let busy = $state(false);
  let message = $state<string | null>(null);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !password) return;
    busy = true;
    message = null;
    try {
      const out = await api.login({
        username: relogin ? undefined : username,
        userId: relogin?.userId,
        password,
        tfaCode: askTfa ? tfaCode : undefined,
      });
      if (out.status === "ok") {
        await app.loadAccounts();
        app.selectedAccount = out.account.user_id;
        onclose();
        return;
      }
      if (out.code === "tfa_required" || out.code === "tfa_invalid") {
        askTfa = true;
        tfaCode = "";
      }
      // Texts the server added are shown as they are, after ours.
      message = [t(`deny.${out.code}` as Key), ...out.errors.filter((x) => x !== "")].join(" ");
      password = "";
    } catch (err) {
      message = errorText(err);
      password = "";
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={relogin ? t("login.reloginTitle", { name: relogin.username }) : t("login.title")}
  {onclose}
>
  <form onsubmit={submit}>
    {#if !relogin}
      <input
        type="text"
        placeholder={t("login.name")}
        bind:value={username}
        autocomplete="username"
        spellcheck="false"
      />
    {/if}
    <input
      type="password"
      placeholder={t("login.password")}
      bind:value={password}
      autocomplete="current-password"
    />
    {#if askTfa}
      <input
        type="text"
        placeholder={t("login.tfa")}
        bind:value={tfaCode}
        inputmode="numeric"
        autocomplete="one-time-code"
      />
    {/if}
    {#if message}<div class="error small">{message}</div>{/if}
    <div class="actions">
      <button
        class="btn"
        type="submit"
        disabled={busy || !password || (!relogin && !username.trim())}
      >
        {busy ? t("login.submitting") : t("login.submit")}
      </button>
      <button class="btn" type="button" onclick={onclose}>{t("login.cancel")}</button>
    </div>
    <p class="sub small">{t("login.privacy")}</p>
  </form>
</Modal>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .small {
    font-size: 13px;
  }
  p {
    margin: 4px 0 0;
  }
</style>
