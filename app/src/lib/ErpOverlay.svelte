<script lang="ts">
  import { clock, promo } from "./rofl.svelte";
  import { app } from "./store.svelte";

  const DEADLINE = 5 * 60;

  let left = $state(DEADLINE);
  let picked = $state<string | null>(null);
  let input = $state<HTMLInputElement>();

  // Never cover a real connection or download: the joke closes itself.
  $effect(() => {
    if (app.isGameBusy && promo.open) promo.open = false;
  });

  // Counts the five minutes down while the window is open.
  $effect(() => {
    if (!promo.open) return;
    left = DEADLINE;
    picked = null;
    const timer = setInterval(() => (left = Math.max(0, left - 1)), 1000);
    return () => clearInterval(timer);
  });

  function chosen(e: Event & { currentTarget: HTMLInputElement }) {
    // Only the name is looked at: the file itself is never read, sent or saved.
    picked = e.currentTarget.files?.[0]?.name ?? null;
    e.currentTarget.value = "";
  }
</script>

<svelte:window onkeydown={(e) => promo.open && e.key === "Escape" && (promo.open = false)} />

{#if promo.open}
  <div class="backdrop" role="alertdialog" aria-modal="true" aria-label="Проверка на возраст">
    <div class="box">
      {#if picked === null}
        <h1>ЗДРАВСТВУЙТЕ!</h1>
        <p class="lead">ВАМ НЕ СТОИЛО БЫЛО ЭТО НАЖИМАТЬ.</p>
        <p class="lead">
          ЭТО ПРОВЕРКА НА ВОЗРАСТ — ВЫ ДОЛЖНЫ СКИНУТЬ ПАСПОРТ В ТЕЧЕНИЕ 5 МИНУТ, ИНАЧЕ:
        </p>
        <ul>
          <li>ДОСТУП К ЛАУНЧЕРУ БУДЕТ ЗАБЛОКИРОВАН</li>
          <li>ДАННЫЕ ОТПРАВЯТСЯ К ФСБ И ПОЛИЦИИ ЗА РАЗВРАТ НЕСОВЕРШЕННОЛЕТНИХ</li>
          <li>ВАШ АККАУНТ БУДЕТ УКРАДЕН И ДАЛЬНЕЙШЕ ЗАБЛОКИРОВАН</li>
        </ul>
        <div class="clock" class:out={left === 0}>{clock(left).slice(3)}</div>
        <input type="file" accept="image/*" hidden bind:this={input} onchange={chosen} />
        <button class="btn send" onclick={() => input?.click()}>ОТПРАВИТЬ</button>
      {:else}
        <h1>ФАЙЛ ПОЛУЧЕН</h1>
        <p class="lead">{picked}</p>
        <p class="lead">ПРОВЕРКА ПРОЙДЕНА. ВАМ ТОЧНО БОЛЬШЕ 18. НАВЕРНОЕ.</p>
        <p class="lead">ПАНЕЛЬ КУПЛЕНА. ДЕНЕГ НЕ СПИСАНО. ПАНЕЛИ НЕТ.</p>
      {/if}

      <p class="joke">
        {picked === null
          ? "это шутка. всё отправится на локальный сервер, и удалится в ближайшее время."
          : "это шутка. файл никуда не отправлялся и нигде не сохранён: лаунчер его даже не открывал."}
      </p>
      <button class="link close" onclick={() => (promo.open = false)}>закрыть</button>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    /* Below ConnectingOverlay (30): a joke must not cover real progress or Cancel. */
    z-index: 20;
    display: grid;
    place-items: center;
    padding: 14px;
    background: #000000e6;
    animation: backdrop-in 0.2s ease-out;
  }
  .box {
    width: 100%;
    height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 20px;
    border: 2px solid var(--bad);
    border-radius: 12px;
    background: #140505;
    color: #ff3b3b;
    text-align: center;
    animation: pop-in 0.2s ease-out;
  }
  h1 {
    color: #ff3b3b;
    font-size: 40px;
    letter-spacing: 0.04em;
  }
  .lead {
    margin: 0;
    font-size: 19px;
    font-weight: 700;
    max-width: 640px;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-weight: 700;
    font-size: 16px;
  }
  .clock {
    font-size: 44px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    animation: blink 1s steps(2, start) infinite;
  }
  .clock.out {
    animation-duration: 0.4s;
  }
  .send {
    min-height: 44px;
    padding: 0 40px;
    border: 2px solid #ff3b3b;
    background: transparent;
    color: #ff3b3b;
    font-size: 18px;
    font-weight: 700;
    letter-spacing: 0.06em;
  }
  .send:hover:not(:disabled) {
    background: #ff3b3b;
    color: #140505;
  }
  .joke {
    margin: 10px 0 0;
    font-size: 13px;
    color: var(--sub);
  }
  .close {
    font-size: 13px;
    color: var(--sub);
  }
  @keyframes blink {
    50% {
      opacity: 0.35;
    }
  }
</style>
