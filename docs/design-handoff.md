# Дизайн: с чего начать (передача работы)

Точка входа для работы над внешним видом. Файл в репозитории (`docs/**/*.md` не игнорируется). Сверено с кодом 2026-11-07.

## Как смотреть интерфейс

```powershell
cd app
pnpm dev          # http://127.0.0.1:1420, вместо бэкенда — src/dev-mock.ts
```

- Мок включается только при `import.meta.env.DEV` и вне Tauri (`app/src/main.ts`). Данные: 7 серверов, 2 аккаунта, 2 пакета, новости, диагностика, журнал.
- Окно приложения: по умолчанию 1000x650, минимум 800x500 (`tauri.conf.json`). В панели браузера удобно 900x600.
- Настоящий бэкенд: `pnpm dev` + `cargo build -p bebra-app` + `target/debug/bebra-app.exe`. Обычный `cargo build` не вшивает фронтенд — окно грузит `127.0.0.1:1420`.
- Отладочный порт WebView2: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, дальше CDP по WebSocket.
- Перед коммитом: `pnpm check`, `pnpm test` (в `app/`), `cargo clippy --workspace --all-targets -- -D warnings` (из корня).

## Дизайн (редизайн 2026-10-04)

- Стиль: плоские поверхности, тон вместо линий, скругления 6/8/12 px, один акцент, тонкие скроллбары, лёгкие анимации (вкладки, диалоги, спиннер).
- Тема по умолчанию — `dark` (графит + синий акцент); прежняя «официальная» — `classic`. Градиент и узор по умолчанию выключены (`settings.rs`).
- Расположение (как в официальном лаунчере):
  - шапка: логотип слева, справа Discord/Сайт над аккаунтом, под шапкой линия акцента (`Header.svelte`);
  - `TabBar` внизу окна (рисуется в `App.svelte` после `main`);
  - в вкладке серверов поиск, фильтры и обновление внизу; панель фильтров (`FiltersPanel`) над ними.
- Настройки: карточки-секции и переключатели вместо флажков (`Option.svelte` — чекбокс показан как switch).
- Точка на вкладке «Обновления» в `TabBar` — когда `app.updateAvailable`; на вкладке «Новости» — когда `app.ownNewsUnseen > 0`.

## Темы и токены

- `lib/themes.ts`:
  - всего 18 id в `THEME_IDS`: 17 встроенных тем + `custom`;
  - `PALETTES` — 8 палитр (`dark`, `classic`, `light`, `dark_red`, `dark_purple`, `midnight_blue`, `emerald_dusk`, `copper_night`);
  - `DERIVED` — 9 тем, заданы четырьмя цветами (`nord`, `dracula`, `rose`, `amber`, `lagoon`, `forest`, `mono`, `sand`, `sky`); остальное считает `deriveCustom`;
  - `custom` — своя тема (`DEFAULT_CUSTOM`);
  - `backgroundFill` (градиент), `stripePattern` (узор), `applyTheme` ставит CSS-переменные из `VARIABLES`.
- Переменные темы (меняются): `--bg --popup --fg --muted --accent --sub --ctrl-mid --ctrl-high --btn-hover --border-mid --border-high --scroll-sep --stripe-edge --on-accent --bg-fill --decor --overlay --font`, плюс устаревшие `--tab-selected --tab-hover --sep --sep-fade --alt-row` (в `var()` не читаются нигде, только объявлены; проверено 2026-11-07).
- Производные в `app.css` (считаются от цвета текста): `--tint`, `--tint-hover`, `--line` (= `--scroll-sep`), `--accent-soft`, `--radius` (8px), `--radius-sm` (6px), `--shadow`. `--page-surface` — полупрозрачный фон `.shell` (страница).
- Постоянные цвета: `--good #57c969`, `--warn #e8c547`, `--bad #e46363` (пинг, ошибки).
- Не вшивать цвета в компоненты: брать переменные, тогда работают все темы.

## Компоненты и классы (`app/src/app.css`)

- Кнопки: `.btn` + модификатор `.primary` / `.soft` / `.ghost` / `.on`; `.icon` — кнопка с иконкой. Склейка кнопок: `.btn.open-r`, `.btn.open-l` (срез с одной стороны), `.btn.open-both` (без срезов).
- Блоки: `.card` (карточка вкладки «Обновления»), `.rule` (разделитель).
- Прочее: `.link`, `.spinner`, `.good` / `.warn` / `.error`, `.sub`, `.numeric`, `.ping`, `.stats`.

## Локализация

- `lib/locales/{ru,en,uk,de}.ts`. `ru` — эталон, типы берутся из него. `i18n.test.ts` требует, чтобы в каждом языке были те же ключи и плейсхолдеры, что в `ru`.
- `lib/languages.ts`: `UI_LANGUAGES`, `nativeName` (название языка на нём самом), `detectLang`.
- Чтобы добавить язык: файл в `locales/`, код в `UI_LANGUAGES`, строка в `dictionaries` (`i18n.svelte.ts`), названия регионов в `REGIONS` (`filters.ts`), локаль даты в `DATE_LOCALE` (`format.ts`).
- Коды языков (`ru-RU`, `RU-ru`) не показываем: `nativeName` — для фильтра языков, тегов сервера и пакетов.
- Строки UI только через `t("ключ", {…})`.

## Где что лежит (`app/src`)

| Файл | За что отвечает |
| --- | --- |
| `app.css` | глобальные переменные, кнопки, поля, скроллбары, `.card`, `.rule` |
| `lib/themes.ts` | палитры, темы, `deriveCustom`, градиент, узор, `applyTheme` |
| `lib/store.svelte.ts` | состояние: вкладка, фильтры, сортировка, серверы, пинги, аккаунты, подключение, настройки (автосохранение через `updateSettings`), `app.update` |
| `lib/filters.ts` | поиск, фильтры (коды как в официальном лаунчере), сортировка, регионы и языки |
| `lib/format.ts` | текст раунда, игроки, цвет пинга, `gridTemplate` (колонки таблицы), размеры, время |
| `lib/i18n.svelte.ts`, `lib/locales/*`, `lib/languages.ts` | строки, языки, `t()` |
| `lib/speed.ts` | `SpeedMeter`: скорость (бит/с) по скользящему окну 3 с |
| `App.svelte` | каркас: Header, вкладка, TabBar, оверлей подключения, диалоги, Toast |
| `lib/Header.svelte`, `Logo.svelte`, `AccountDropdown.svelte` | шапка, логотип (HTML/CSS: квадрат «B» + текст, не SVG), список аккаунтов |
| `lib/TabBar.svelte` | вкладки внизу окна |
| `lib/ServersTab.svelte`, `FavoritesTab.svelte`, `RecentTab.svelte`, `NewsTab.svelte`, `OptionsTab.svelte`, `ModsTab.svelte`, `UpdatesTab.svelte` | вкладки |
| `lib/ServerTable.svelte`, `ServerEntry.svelte`, `FiltersPanel.svelte` | таблица, строка-экспандер, фильтры |
| `lib/RoundTimer.svelte` | колонка «Раунд»: `setInterval` 1 с на каждую строку |
| `lib/ConnectingOverlay.svelte`, `Toast.svelte`, `Modal.svelte`, `DialogHost.svelte` | окно подключения, уведомления, модалки |
| `lib/*Dialog.svelte` | `ConfirmConnectDialog`, `LoginDialog`, `DirectConnectDialog`, `AddFavoriteDialog`, `EditFavoriteDialog` |
| `lib/setup/*` | мастер первой настройки на всё окно (`SetupWizard.svelte`, шаги `Step*.svelte`, `steps.ts` со списком шагов и `SETUP_VERSION`, `setup.css`, `context.svelte.ts`); Escape и Tab его не закрывают, перепрохождение — «Настройки» → «О программе» |
| `lib/Icon.svelte`, `Option.svelte` | иконки (пути SVG), строка-настройка |
| `lib/api.ts` | вызовы Tauri-команд |
| `dev-mock.ts` | тестовый бэкенд для браузера |
| `lib/changelog.ts`, `lib/UpdatesTab.svelte` | журнал версий и вкладка «Обновления» (событие `update` от бэкенда) |
| не разобрано | `SpaceBackground.svelte` — в этом документе не описан |

Референсы вида: официальный лаунчер (`..\Forks\space-wizard SS14 launcher\SS14.Launcher\Theme\*.xaml`, `Views\*`) и MusyaLoader (`..\Forks\MusyaLoader\...\AppThemeManager.cs`, `OptionsTabView.xaml`). Репозиторий `Bebranot/BebraLauncher` публичный (с 2026-11-07); MusyaLoader под AGPL, при заимствовании кода проверять лицензию.

## Что в дизайне ещё не доделано

- Иконка приложения: заглушка «B» (`app/src-tauri/icons/source.png`; пересоздать через `pnpm tauri icon`).
- Логотип в шапке — простой HTML/CSS; официальный лаунчер использует картинку.
- Своей заголовочной панели нет: стандартная рамка Windows (`tauri.conf.json` без `decorations`).
- Узор `--decor` почти не виден на тёмных темах: можно поднять контраст.
- Свою тему и палитры, кроме `dark`/`light`, после редизайна глазами не проверяли целиком.
- Проверить светлую и «свою» тему на всех экранах (ConnectingOverlay, диалоги, Toast).
- Колонка «Раунд» тикает раз в секунду в каждой строке; при сотнях серверов можно ограничить видимыми.
- Таблица без виртуализации: 180 строк работают, 1000+ могут тормозить.

## Вкладка «Обновления»

- `lib/UpdatesTab.svelte`: сверху полоса состояния (тон зависит от состояния, не прокручивается), ниже слева история изменений, справа настройки; состояние — `app.update` (`store.svelte.ts`), события `update` от бэкенда (`selfupdate.rs`), разбор журнала — `lib/changelog.ts`.
- В `dev-mock.ts` команда `check_update` проигрывает весь путь: найдено, загрузка, готово.
- На минимальной ширине 800 px шапка помещается впритык: новые вкладки и длинные слова проверять в каждом языке.

## Вкладка «Новости»

- Два источника: блог SS14 (`news`) и свои новости (`own_news`: файл `news/news.json` в репозитории, бэкенд читает его в `news.rs`; как писать: `news/README.md`). Прочитанные id хранятся в `localStorage` (ключ `bebra.news.seen`, `store.svelte.ts`); `lib/news.ts` выбирает текст языка (`localized`) и непрочитанные (`unseen`).

## Ловушки

- `$effect`, который читает и пишет одни и те же данные, зацикливает страницу и вешает вкладку (был баг с пингом). Внутри — `untrack` (так сделано в `store.svelte.ts`, `ConnectingOverlay.svelte`, `LoginDialog.svelte`, `NewsTab.svelte`).
- Svelte-компонент без логики держит пустой `<script lang="ts"></script>` (см. `Logo.svelte`) — иначе нет типов для TS.
- `export type` в компоненте — только в `<script module>` (правило Svelte; в репозитории такого нет).
- Scoped CSS: одинаковые имена классов у разных элементов одного компонента конфликтуют (был баг с `.empty`).
- CSP (`tauri.conf.json`): `default-src 'self'`, `connect-src ipc: http://ipc.localhost`, `img-src 'self' data:`, `font-src 'self' data:`, `style-src 'self' 'unsafe-inline'`; внешние ресурсы не грузятся.
