<script lang="ts">
  import { api, errorText } from "./api";
  import { formatDate } from "./format";
  import { t, type Key } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let { onclose }: { onclose: () => void } = $props();

  // Logging in: a new name, or an existing account that has to log in again (userId).
  let username = $state("");
  let relogin = $state<{ userId: string; username: string } | null>(null);
  let password = $state("");
  let tfaCode = $state("");
  let askTfa = $state(false);
  let busy = $state(false);
  let message = $state<string | null>(null);

  function startRelogin(userId: string, name: string) {
    relogin = { userId, username: name };
    username = name;
    reset();
  }

  function reset() {
    password = "";
    tfaCode = "";
    askTfa = false;
    message = null;
  }

  function cancelRelogin() {
    relogin = null;
    username = "";
    reset();
  }

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
        username = "";
        relogin = null;
        reset();
      } else {
        if (out.code === "tfa_required" || out.code === "tfa_invalid") {
          askTfa = true;
          tfaCode = "";
        }
        // Texts the server added are shown as they are, after ours.
        message = [t(`deny.${out.code}` as Key), ...out.errors.filter((x) => x !== "")].join(" ");
        password = "";
      }
    } catch (err) {
      message = errorText(err);
      password = "";
    } finally {
      busy = false;
    }
  }

  async function logout(userId: string) {
    try {
      const told = await api.logout(userId);
      await app.loadAccounts();
      if (!told) message = t("acc.offline");
    } catch (err) {
      message = errorText(err);
    }
  }
</script>

<Modal title={t("acc.title")} {onclose}>
  {#if app.accounts.length > 0}
    <ul class="accounts">
      {#each app.accounts as a (a.user_id)}
        <li>
          <div class="who">
            <b>{a.username}</b>
            <span class="dim small" class:error={a.state === "expired"}>
              {a.state === "expired"
                ? t("acc.expired")
                : t("acc.valid", { date: formatDate(a.expire_time) })}
            </span>
          </div>
          {#if a.state === "expired"}
            <button onclick={() => startRelogin(a.user_id, a.username)}>{t("acc.relogin")}</button>
          {/if}
          <button class="danger" onclick={() => logout(a.user_id)}>{t("acc.logout")}</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="dim">{t("acc.empty")}</p>
  {/if}

  <form onsubmit={submit}>
    <h3>{relogin ? t("acc.reloginTitle", { name: relogin.username }) : t("acc.add")}</h3>
    {#if !relogin}
      <input
        type="text"
        placeholder={t("acc.name")}
        bind:value={username}
        autocomplete="username"
      />
    {/if}
    <input
      type="password"
      placeholder={t("acc.password")}
      bind:value={password}
      autocomplete="current-password"
    />
    {#if askTfa}
      <input
        type="text"
        placeholder={t("acc.tfa")}
        bind:value={tfaCode}
        inputmode="numeric"
        autocomplete="one-time-code"
      />
    {/if}
    {#if message}<div class="error small">{message}</div>{/if}
    <div class="actions">
      <button
        class="primary"
        type="submit"
        disabled={busy || !password || (!relogin && !username.trim())}
      >
        {busy ? t("acc.submitting") : t("acc.submit")}
      </button>
      {#if relogin}
        <button type="button" onclick={cancelRelogin}>{t("acc.cancel")}</button>
      {/if}
    </div>
    <p class="dim small">{t("acc.privacy")}</p>
  </form>
</Modal>

<style>
  .accounts {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .who {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .small {
    font-size: 12px;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  p {
    margin: 0;
  }
</style>
