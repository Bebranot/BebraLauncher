<script lang="ts">
  import { clock, formatBytes, promo, uploadSeconds } from "./rofl.svelte";
  import { app } from "./store.svelte";

  const DEADLINE = 5 * 60;

  /** The "verification" is a staged joke: pick a file, watch it "upload", get a verdict. */
  type Phase = "form" | "uploading" | "review" | "done";

  let left = $state(DEADLINE);
  let phase = $state<Phase>("form");
  let caseNo = $state("");
  let fileName = $state("");
  let fileSize = $state(0);
  let fileKind = $state("");
  let progress = $state(0);
  let sent = $state(0);
  let speed = $state(0);
  let input = $state<HTMLInputElement>();

  const percent = $derived(Math.round(progress * 100));
  const eta = $derived(Math.max(0, Math.ceil((fileSize - sent) / Math.max(1, speed))));

  let tick: ReturnType<typeof setInterval> | null = null;
  let review: ReturnType<typeof setTimeout> | null = null;

  // Never cover a real connection or download: the joke closes itself.
  $effect(() => {
    if (app.isGameBusy && promo.open) promo.open = false;
  });

  // Each opening starts a fresh "request" with its own number and countdown.
  $effect(() => {
    if (!promo.open) return;
    left = DEADLINE;
    phase = "form";
    fileName = "";
    fileSize = 0;
    fileKind = "";
    progress = 0;
    sent = 0;
    speed = 0;
    caseNo = `${new Date().getFullYear()}-${String(100000 + Math.floor(Math.random() * 900000))}`;
    const timer = setInterval(() => (left = Math.max(0, left - 1)), 1000);
    return () => {
      clearInterval(timer);
      stopTimers();
    };
  });

  function stopTimers() {
    if (tick !== null) {
      clearInterval(tick);
      tick = null;
    }
    if (review !== null) {
      clearTimeout(review);
      review = null;
    }
  }

  function kindOf(file: File): string {
    const ext = file.name.includes(".") ? file.name.split(".").pop() : "";
    if (ext) return ext.toUpperCase();
    const sub = file.type.split("/")[1];
    return sub ? sub.toUpperCase() : "ФАЙЛ";
  }

  function chosen(e: Event & { currentTarget: HTMLInputElement }) {
    // Only the name, size and type are read: the file itself is never opened, sent or saved.
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = "";
    if (!file) return;
    fileName = file.name;
    fileSize = file.size;
    fileKind = kindOf(file);
    beginUpload();
  }

  /** A fake upload whose duration follows the real file size, as a real client would. */
  function beginUpload() {
    stopTimers();
    phase = "uploading";
    progress = 0;
    sent = 0;
    const seconds = uploadSeconds(fileSize);
    speed = Math.round(fileSize / seconds);
    const started = performance.now();
    tick = setInterval(() => {
      const elapsed = (performance.now() - started) / 1000;
      const next = Math.min(1, elapsed / seconds);
      progress = Math.max(progress, next);
      sent = Math.round(fileSize * progress);
      if (next < 1) {
        // The speed wobbles a little, as a real connection does.
        speed = Math.max(1, Math.round((fileSize / seconds) * (0.82 + Math.random() * 0.36)));
        return;
      }
      progress = 1;
      sent = fileSize;
      stopTimers();
      phase = "review";
      review = setTimeout(() => {
        phase = "done";
        review = null;
      }, 2400);
    }, 120);
  }
</script>

{#if promo.open}
  <div class="backdrop" role="alertdialog" aria-modal="true" aria-label="Проверка возраста">
    <div class="box">
      <div class="doc-head">
        <div class="emblem" aria-hidden="true">18+</div>
        <div class="doc-title">
          <strong>ЕДИНАЯ СИСТЕМА ВЕРИФИКАЦИИ ВОЗРАСТА</strong>
          <span>Заявка № {caseNo} · ЕРП-панель «Premium»</span>
        </div>
        <div class="status">защищено</div>
      </div>

      <ol class="steps">
        <li class:active={phase === "form"} class:done={phase !== "form"}>Документ</li>
        <li class:active={phase === "uploading" || phase === "review"} class:done={phase === "done"}>
          Проверка
        </li>
        <li class:active={phase === "done"}>Результат</li>
      </ol>

      {#if phase === "form"}
        <h1>Подтвердите возраст</h1>
        <p class="lead">
          Для активации ЕРП-панели нужно убедиться, что вам исполнилось 18 лет.
        </p>
        <p class="body">
          Загрузите фото или скан первой страницы паспорта. Проверка автоматическая и занимает
          не больше пяти минут.
        </p>
        <ul class="threats">
          <li>Доступ к ЕРП-панели будет заблокирован.</li>
          <li>Заявка будет передана в службу контроля.</li>
          <li>Аккаунт будет временно ограничен.</li>
        </ul>
        <div class="deadline" class:over={left === 0}>
          <span>{left === 0 ? "СРОК ИСТЁК" : "ДО ИСТЕЧЕНИЯ СРОКА"}</span>
          <b>{left === 0 ? "00:00" : clock(left).slice(3)}</b>
        </div>
        {#if left === 0}
          <p class="body warn-text">
            Срок истёк, но документ всё ещё можно загрузить: проверка займёт больше времени.
          </p>
        {/if}
        <input
          type="file"
          accept="image/*,.pdf"
          hidden
          bind:this={input}
          onchange={chosen}
        />
        <div class="actions">
          <button class="send" onclick={() => input?.click()}>ЗАГРУЗИТЬ ДОКУМЕНТ</button>
          <button class="link" onclick={() => (promo.open = false)}>отменить</button>
        </div>
        <p class="transport">Принимаются JPEG, PNG, HEIC и PDF · соединение защищено</p>
      {:else if phase === "uploading"}
        <h1>Отправка документа</h1>
        <div class="file">
          <span class="fname">{fileName}</span>
          <span class="fmeta">{formatBytes(fileSize)} · {fileKind}</span>
        </div>
        <div class="bar" aria-hidden="true">
          <div class="fill" style:width={`${percent}%`}></div>
        </div>
        <div class="bar-row" aria-live="polite">
          <b>{percent}%</b>
          <span>{formatBytes(sent)} из {formatBytes(fileSize)}</span>
        </div>
        <div class="stats">
          <div><span>Скорость</span><b>{formatBytes(speed)}/с</b></div>
          <div><span>Осталось</span><b>{eta} с</b></div>
        </div>
        <p class="body">Не закрывайте окно: при отмене заявка аннулируется.</p>
        <div class="actions">
          <button class="link" onclick={() => (promo.open = false)}>отменить отправку</button>
        </div>
        <p class="transport">TLS 1.3 · AES-256 · документ передаётся в зашифрованном виде</p>
      {:else if phase === "review"}
        <h1>Документ принят</h1>
        <p class="body">Проверяем подлинность и читаемость документа…</p>
        <ul class="checks">
          <li>Формат файла распознан</li>
          <li>Размер в пределах нормы</li>
          <li>Лицо и данные читаются</li>
          <li>Возраст подтверждён</li>
        </ul>
        <p class="transport">{formatBytes(fileSize)} · {fileKind} · SHA-256 проверен</p>
      {:else}
        <h1 class="ok">Проверка пройдена</h1>
        <p class="body">
          Вам больше 18. Оплата прошла: 149 ₽ списаны с вашего счёта.
        </p>
        <div class="result fail">
          <span>ЕРП-ПАНЕЛЬ «PREMIUM»</span>
          <b>НЕ КУПЛЕНА</b>
        </div>
        <div class="punch">
          <p class="punch-title">
            149 ₽ СПИСАНЫ.<br />
            ПАНЕЛИ НЕТ.
          </p>
          <p class="body">
            Деньги ушли на развитие лаунчера. Спасибо за поддержку!
          </p>
        </div>
        <div class="actions">
          <button class="send" onclick={() => promo.markBought()}>ЗАКРЫТЬ</button>
        </div>
      {/if}
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
    background: #05070ae6;
    backdrop-filter: blur(2px);
    animation: backdrop-in 0.18s ease-out;
  }
  .box {
    width: min(100%, 620px);
    max-height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 0 22px 20px;
    border: 1px solid #2b3542;
    border-radius: 10px;
    background: #0c1015;
    color: #d7dde6;
    box-shadow: 0 24px 80px #000000cc;
    animation: pop-in 0.18s ease-out;
  }

  .doc-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0 -22px;
    padding: 14px 22px;
    border-bottom: 1px solid #232c37;
    border-radius: 10px 10px 0 0;
    background: #0a0d12;
  }
  .emblem {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border: 2px solid #d64545;
    border-radius: 6px;
    color: #ff5c5c;
    font-size: 16px;
    font-weight: 700;
  }
  .doc-title {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .doc-title strong {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.07em;
    line-height: 1.3;
  }
  .doc-title span {
    font-size: 11px;
    color: var(--sub);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    flex: none;
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--good);
  }
  .status::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--good);
    box-shadow: 0 0 6px var(--good);
  }

  .steps {
    display: flex;
    gap: 6px;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    counter-reset: step;
  }
  .steps li {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 2px solid #232c37;
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    counter-increment: step;
  }
  .steps li::before {
    content: counter(step);
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: 1px solid currentColor;
    border-radius: 50%;
    font-size: 10px;
  }
  .steps li.active {
    border-bottom-color: #d64545;
    color: #f0f3f7;
  }
  .steps li.done {
    border-bottom-color: var(--good);
    color: var(--good);
  }

  h1 {
    margin: 6px 0 0;
    color: #f2f5f8;
    font-size: 22px;
    font-weight: 600;
  }
  h1.ok {
    color: var(--good);
  }
  .lead {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: #e8edf3;
  }
  .body {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
    color: #aab3bf;
  }
  .warn-text {
    color: var(--warn);
  }

  .threats {
    margin: 0;
    padding: 10px 12px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 1px solid #3a2222;
    border-left: 3px solid #d64545;
    border-radius: 4px;
    background: #1b0f0f;
    font-size: 13px;
    color: #e0b4b4;
  }
  .threats li::before {
    content: "—";
    margin-right: 7px;
    color: #d64545;
  }

  .deadline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border: 1px solid #232c37;
    border-radius: 4px;
    background: #0f141a;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--sub);
  }
  .deadline b {
    color: #ff5c5c;
    font-size: 26px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
  }
  .deadline.over b {
    animation: blink 0.5s steps(2, start) infinite;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 4px;
  }
  .actions .link {
    color: var(--sub);
    font-size: 13px;
  }
  .send {
    min-height: 44px;
    padding: 0 26px;
    border: 1px solid #d64545;
    border-radius: 6px;
    background: #d64545;
    color: #ffffff;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: background 0.12s ease;
  }
  .send:hover {
    background: #b93a3a;
  }
  .transport {
    margin: 0;
    font-size: 11px;
    color: #6f7885;
  }

  .file {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 12px;
    border: 1px solid #232c37;
    border-radius: 4px;
    background: #0f141a;
  }
  .fname {
    font-size: 13px;
    font-weight: 600;
    color: #e8edf3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fmeta {
    font-size: 11px;
    color: var(--sub);
  }
  .bar {
    height: 8px;
    overflow: hidden;
    border-radius: 4px;
    background: #1b232d;
  }
  .fill {
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, #b33535, #e46363);
    transition: width 0.12s linear;
  }
  .bar-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: 12px;
    color: var(--sub);
  }
  .bar-row b {
    color: #f0f3f7;
    font-size: 15px;
    font-variant-numeric: tabular-nums;
  }
  .stats {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .stats div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 7px 10px;
    border: 1px solid #232c37;
    border-radius: 4px;
    background: #0f141a;
  }
  .stats span {
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--sub);
  }
  .stats b {
    font-size: 14px;
    font-variant-numeric: tabular-nums;
  }

  .checks {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
    color: #cfd6df;
  }
  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    animation: check-in 0.25s ease-out both;
  }
  .checks li:nth-child(1) {
    animation-delay: 0.15s;
  }
  .checks li:nth-child(2) {
    animation-delay: 0.55s;
  }
  .checks li:nth-child(3) {
    animation-delay: 0.95s;
  }
  .checks li:nth-child(4) {
    animation-delay: 1.35s;
  }
  .checks li::before {
    content: "✓";
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--good) 18%, transparent);
    color: var(--good);
    font-size: 11px;
  }

  .result {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--good) 40%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--good) 9%, transparent);
  }
  .result span {
    font-size: 12px;
    letter-spacing: 0.05em;
    color: #cfd6df;
  }
  .result b {
    font-size: 13px;
    letter-spacing: 0.06em;
    color: var(--good);
  }
  .result.fail {
    border-color: color-mix(in srgb, var(--bad) 40%, transparent);
    background: color-mix(in srgb, var(--bad) 9%, transparent);
  }
  .result.fail b {
    color: var(--bad);
  }

  .punch {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
    padding-top: 14px;
    border-top: 1px dashed #2b3542;
  }
  .punch-title {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    line-height: 1.35;
    color: var(--warn);
  }

  @keyframes blink {
    50% {
      opacity: 0.35;
    }
  }
  @keyframes check-in {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
  }
  @keyframes backdrop-in {
    from {
      opacity: 0;
    }
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.985);
    }
  }
  @media (max-width: 520px) {
    .box {
      padding: 0 16px 16px;
    }
    .doc-head {
      margin: 0 -16px;
      padding: 12px 16px;
    }
    .doc-title strong {
      font-size: 10px;
    }
    .status {
      display: none;
    }
    h1 {
      font-size: 20px;
    }
    .steps li {
      font-size: 10px;
    }
  }
</style>
