<script lang="ts">
  import { t } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let address = $state("");
  let invalid = $state(false);
  let input = $state<HTMLInputElement>();

  $effect(() => input?.focus());

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!address.trim()) return;
    // The backend knows what an address looks like; it says no for a bad one.
    const problem = await app.addFavorite(address);
    if (problem) {
      invalid = true;
    } else {
      onclose();
    }
  }
</script>

<Modal title={t("addfav.title")} {onclose}>
  <form onsubmit={submit}>
    <label for="fav-address">{t("addfav.address")}</label>
    <input
      id="fav-address"
      type="text"
      placeholder={t("direct.placeholder")}
      bind:value={address}
      bind:this={input}
      oninput={() => (invalid = false)}
      spellcheck="false"
      autocomplete="off"
    />
    {#if invalid}<div class="error">{t("addfav.invalid")}</div>{/if}
    <div>
      <button class="btn" type="submit" disabled={!address.trim()}>{t("addfav.add")}</button>
    </div>
  </form>
</Modal>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
</style>
