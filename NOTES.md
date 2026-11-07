# NOTES — только важные и тонкие детали

Неочевидное: грабли, решения, нюансы, непроверенное. Остальное — по ссылкам:
протокол `docs/protocol.md`, Loader и отличия от upstream `loader/README.md`, UI и Svelte-ловушки
`docs/design-handoff.md`, версии и журнал `docs/CHANGELOG_RULES.md`, задачи `docs/IMPROVEMENTS.md`, `docs/FIXLOG.md`.
Какие файлы попадают в git — см. `.gitignore`.

Референсы (только чтение, `..\Forks\`): официальный лаунчер space-wizard и MusyaLoader. По ним ссылки `[код: ...]` в `docs/protocol.md`.

Готовая сборка игры, если понадобится: `C:\LocalDutyAVS` (исходники LocalDuty и собранные клиент и сервер: `bin\Content.Client\Content.Client.exe`, `bin\Content.Server\Content.Server.exe`). Лежит вне репозитория, только на этой машине.

## Архитектурные решения

- Игру запускает `SS14.Loader.exe` (.NET 10, `loader/`, из исходников официального лаунчера, MIT). Rust-ядро Loader заменить не может. Почему — `docs/protocol.md`, раздел 9.
- Схема `content.db` совпадает с официальным лаунчером; Loader читает её напрямую. Не менять.
- Токены не попадают в UI: `AccountRow` — только id, имя, срок и состояние. Пароль идёт по IPC только в `login` и сразу на auth-сервер.
- `ss14://` и `ss14s://` (и redial) выбирают сервер и открывают `ConfirmConnectDialog`; подключение — после подтверждения. Официальный лаунчер подключается сразу — отличие сознательное.
  - Регистрация схем: `HKCU\Software\Classes` (`app/src-tauri/src/system.rs`), переключатель в настройках (`get/set_links_registered`).
- Текстовые пакеты: принимаются только `Locale/<culture>/*.ftl`. Любой другой файл — отклоняется весь пакет (`crates/core/src/packs.rs`, любая проблема делает пакет невалидным).
- Моды (`.dll`) — сторонний неподписанный код в процессе игры. Защита — SHA-256 файлов из `patches/manifest.json`, проверка перед каждым запуском (`patches.rs`). Автор и подпись не проверяются. Сервер может счесть моды читерством (см. UI и README).
- Палитры тем скопированы из MusyaLoader (AGPL): репозиторий публичный, нужна проверка лицензии.

## Протокол и сеть

- Хаб отдаёт поля сверх модели официального лаунчера 0.40.1 (`map`, `preset`, `round_id`, `panic_bunker`, `inferredTags`). Парсер мягкий: `deny_unknown_fields` нигде не используется; у `statusData` опционально всё, кроме `players`.
- Порт по умолчанию: `ss14://` — 1212, `ss14s://` — 443 (`addr.rs`).
- `/info` серверов берётся через хаб (`api/servers/info?url=`) и работает, даже если сервер недоступен напрямую. Но подключение всё равно требует `/info` у самого сервера (как и у официального лаунчера): недоступный напрямую сервер не подключится (проверено на eridani).
- Прямой `/status` не отвечает за 5 с (`STATUS_TIMEOUT`) — это штатный Offline, не ошибка.
- Зеркала (`crates/core/src/http.rs`):
  - У базовых URL (хаб, auth, CDN) есть fallback. GET (`send`): второе зеркало стартует через 3 с (`ATTEMPT_DELAY`), параллельно.
  - Не-идемпотентные POST (login, refresh) — только по очереди (`send_ordered`): токен-эндпоинт ротирует токен, два зеркала не должны видеть один запрос.
- Заголовок `SS14-Launcher-Fingerprint` не отправляем (решение владельца; в коде его нет). Если auth начнёт отклонять вход без него — вернуться к вопросу.
- В манифесте нет размеров файлов, поэтому размер delta заранее неизвестен. Прогресс — по блобам; оценку не выдаём за точную.
- Не проверять размер запросом с `Range` и не делать `GET` на zip ради проверки: CDN игнорирует Range. `HEAD` на `client.zip` даёт 405.
- Версия модуля: максимальная `ver ≤ engine_version`, суффиксы вроде `-fix-physics` отбрасываются (`modules.rs`). Официальный лаунчер на таких падает в `System.Version.Parse` — наше отличие.
- Движок с `insecure: true` не запускается; `redirect` — цепочка версий (`engine.rs`).
- Манифестный режим проверен живьём, включая delta-обновление. Zip-режим и ACZ на живом сервере не проверены: все проверенные серверы имеют манифест.
- Блобы пишутся пачками (коммит каждые 200 файлов или 32 МБ, `download.rs`), без транзакции поверх сетевых чтений (P1-5). При обрыве готовые блобы сохраняются в `InterruptedDownload` (+ `InterruptedDownloadContent`) и подхватываются при повторе. Обрывки старше 7 суток (`interrupted_keep_hours`) удаляет cull (`content_db/cull.rs`).

## Подпись и доверие

- Ed25519 проверяется дважды:
  - ядро — сразу после скачивания движка (`engine.rs`) и модулей (`modules.rs`);
  - Loader — при каждом запуске, над engine zip (`loader/SS14.Loader/Program.cs`). Неподписанный движок не стартует.
- Ключ выбирается по имени движка: `signing_key.pem` (Robust), `signing_key_multiverse.pem`, `signing_key_supermatter.pem` (`signature.rs::key_pem_for_engine`). Происхождение двух последних ключей здесь не описано.
- Выключателя подписи нет: `SS14_DISABLE_SIGNING` удалён из Loader. `launch.rs` (`CLEARED_ENV`) убирает эту переменную из окружения клиента. Не возвращать.
- Подпись не покрывает патчи `.dll` (см. «Архитектурные решения»).
- Самообновление (`self_update.rs`):
  - Ed25519 над строкой `bebra-update:<версия>:<sha256 установщика>`; публичный ключ вшит (`update_key.pem`).
  - Закрытый ключ только у владельца: `%USERPROFILE%\.bebra\update-signing.key` (или `$env:BEBRA_UPDATE_KEY`), создаётся `bebra update-keygen`. В git не попадает (`*.key` в `.gitignore`). При потере — вручную поставить версию со сменой ключа.
  - Токен GitHub необязателен: репозиторий публичный, `releases/latest`, установщик и подпись скачиваются без него. Токен (Contents: read) — Credential Manager, цель `BebraLauncher/github-token` (`selfupdate.rs`). Уходит только на `api.github.com` и `github.com` (`TOKEN_HOSTS`, `self_update.rs`), на адреса из ответа GitHub не уходит. Нужен для приватного репозитория; при 404 без токена ошибка подсказывает добавить токен (`explain`).
  - Новости лаунчера (`read_repo_file`): сначала `raw.githubusercontent.com` без токена, токен — только при 404.
- Выпуск: `scripts/release.ps1` (нужен `GITHUB_TOKEN` с правом записи; `-DryRun` собирает и подписывает без публикации). Релизы публикуются, последний — 1.1.000 (2026-11-07). Живой цикл «скачать и поставить» — см. «Не проверено вживую».
- Самообновление проверяет релизы через 8 с после старта и далее каждые 4 ч (`selfupdate.rs`). Установка при закрытии — только если включено автообновление и не идёт игра (`install_on_exit`, `RunEvent::Exit` в `main.rs`). В debug-сборке установка отключена (`cfg!(debug_assertions)`).

## Аккаунты

- Пароль не хранится и не логируется.
- Токен — Credential Manager, цель `BebraLauncher/account/<userId>`, `Persist=LOCAL_MACHINE`, blob — JSON `StoredAccount`, лимит 2560 байт (`accounts.rs`). Любой процесс того же пользователя его прочитает: это не сильнее самой учётной записи Windows.
- Клиенту токен передаётся только через env `ROBUST_AUTH_*`, и только если `auth.mode != Disabled` и аккаунт выбран (`play.rs`, `launch.rs`). В лог команда пишется без значений env (`ROBUST_AUTH_TOKEN=<hidden>`).
- `auth.mode=Required` без аккаунта (и без `--no-connect`) — `PlayError::AuthRequired` (`play.rs::check_auth`).
- Продление — если до истечения меньше 15 суток (`REFRESH_THRESHOLD`). Сетевая ошибка не означает мёртвый токен, пока он не истёк по времени. 401 на refresh или ping — токен мёртв.
- `token_refresh_lock` (`game.rs`) не даёт login/logout/select гоняться с продлением.
- Гость: при `auth.mode=Disabled` или без выбранного аккаунта имя `JoeGenero` (`FALLBACK_USERNAME`).
- Живьём проверено 2026-10-03: вход через `bebra login`, ping с мусорным токеном даёт 401, подключение к серверу с `auth.mode=Required`.

## Запуск игры

- Loader сам пишет и удаляет строку `RunningClient` и держит read-транзакцию на `content.db` (`loader/SS14.Loader/ContentDbFileApi.cs`). Loader проверяет `SQLITE_DONE` при записи строки, иначе живой клиент будет считаться мёртвым.
- Cull считает клиент живым, если есть строка `RunningClient`, PID жив и путь exe совпадает (`content_db/cull.rs`).
- Cull: не больше 15 версий всего и 3 на один форк (`CullPolicy`). Версия в использовании или с `RunningClient` не удаляется.
- Клиент при ошибке подключения остаётся открытым (строка `RunningClient` жива), пока пользователь не закроет окно.
- `--launcher` подразумевает `--connect` (`launch.rs`). Build info (`engine_version`, `manifest_hash`) передаётся, только если сервер его дал.
- Из пользовательских аргументов проходит только `--cvar name=value`; `--connect-address`, `--username`, `--ss14-address` отбрасываются (`safe_extra_args`).
- Redial: Loader запускает `$SS14_LAUNCHER_PATH --commands :RedialWait R<hex> C<hex>` (`RedialApi.cs`). Лаунчер принимает команду через single-instance (`main.rs`); если лаунчер не был запущен — из argv (`redial.rs`). Вживую не проверено: нужен сервер, который шлёт redial.
- Время игры и история пишутся только при событии `Running` (`game.rs`). Незаконченные сессии — в `active_sessions.json` (`sessions.rs`).
- Отладка без подключения: `bebra launch <ss14s://...> --no-connect` (`crates/cli`) открывает главное меню клиента. Гостем на `auth.mode=Required` не войти.

## Настройки и файлы

- Данные: `%LOCALAPPDATA%\BebraLauncher` (`paths.rs`; portable — если рядом с exe есть `portable.txt` или `.portable`, то `<exe>\data`). Внутри: `content.db`, `engines\`, `modules\`, `patches\`, `overlays\`, `logs\`. Отдельно от официального лаунчера.
- Новые поля `settings.json` объявляются с `#[serde(default)]`; санитайзинг — `Settings::sanitized` (`settings.rs`).
- Битый `settings.json`: берётся `.bak`, битый файл переименовывается в `settings.json.corrupt-<ts>`.
- `merged_from_ui` сливает поля из UI с текущими в Rust, поэтому UI не затирает `favorites` и `history`. `hwid_current` и `hwid_previous` пишет только лаунчер.
- `last_account`: `None` — не выбирал; `""` — гость явно; иначе user id.
- `logs/launcher.log` ротируется при 1 МБ (`applog.rs`).
- После живых тестов откатить историю и `last_account` в `settings.json`: там могут остаться записи.

## Мастер первой настройки и импорт

- Мастер (`app/src/lib/setup/`) открывается, пока `settings.setup_version` меньше `SETUP_VERSION` (`steps.ts`) и `CURRENT_SETUP_VERSION` (`settings.rs`). Значения в обоих местах держать равными. Чтобы показать мастер всем после обновления, поднять оба на 1 и дописать шаги.
- Первый запуск закрыть нельзя: ни Escape, ни крестика. Перепрохождение (`app.setupRerun`) закрывается. Настройки применяются сразу, `finishSetup` лишь записывает версию.
- Импорт из официального лаунчера (`crates/core/src/official.rs`) только читает его базу: избранное, хабы, режим совместимости. Токены входа не переносятся. Шаг показывается, только если база найдена (`stepsFor(officialFound)`).
- В браузере с `dev-mock.ts` мастер пропускается параметром `?setup=done`.

## HWID

- Зачем: серверы различают компьютер по HWID; вычисленный движком идентификатор иногда непригоден (подтвердил админ сервера).
- `hwid.rs`: 32 случайных байта → `legacy` (64 hex) и `modern` (`00` + те же байты, 66 hex). Передаются в env `BEBRA_HWID_LEGACY` и `BEBRA_HWID_MODERN`.
- Патч `loader/Bebra.HwidPatch/` (Harmony) постфиксит `GetLegacy` и `GetModern`. Типы движка ищутся по имени (`AccessTools.TypeByName`), поэтому смена версии Robust не ломает сборку патча.
- `Bebra.HwidPatch.dll` ищется рядом с `SS14.Loader.exe` (`launch.rs`). Если HWID задан, а файла нет, UI пишет предупреждение (`commands.rs`).
- `hwid_auto` включён по умолчанию: новый идентификатор перед каждым запуском.
- Не проверено: смена HWID на живом сервере.

## Оптимизация

- `launch::OptimizationConfig` — 18 независимых переключателей; пресеты (`standard`/`balanced`/`optimized`) — наборы этих же флагов (`OptimizationConfig::preset`). «custom» хранится в `Settings::opt_custom`; старые `opt_fast_lighting`/`opt_unlock_fps`/`opt_high_priority`/`opt_server_gc` читаются один раз в `sanitized` и больше не пишутся.
- Значения cvar сверены с кодом игры (`C:\LocalDutyAVS`): `light.resolution_scale` по умолчанию 0.5 (поэтому 0.35), `particles.quality` 3, `audio.tick_rate` 30. `display.force_sync_windows`, `midi.parallelism`, `light.max_occluder_count` в релизной сборке ничего не дают; `light.max_light_count=256` даёт мерцание света — не возвращать.
- Замеры: `scripts/bench/README.md`, результаты — `docs/performance-report.md`. Коротко, чтобы не проверять заново: оконный VSync движка (`DwmFlush` после каждого swap, `Sdl3.Window.cs`) на Vega 11 даёт 23 FPS, полный экран не спасает, а `display.vsync=false` + `display.max_fps=60` держит 59. `res.rsi_atlas_size` меньше только снижает FPS и память не экономит. `res.texturepreloadingenabled=false` экономит ~200 МБ, но на SSD нельзя проверить подгрузку текстур посреди игры, поэтому в профилях его нет. `DOTNET_TieredPGO=0` теряет 13% FPS. `DOTNET_ReadyToRun=1` даёт только −2 с старта. Workstation GC с большим gen0 даёт более длинные паузы. `light.resolution_scale` ниже 0.35, `max_shadowcasting_lights`, частицы, параллакс и звук поверх «Оптимизации» в пределах шума, новый профиль «ещё слабее» поэтому не добавлен.
- `PatchManager.SetStaticField` пишет и в поле, и в свойство: наши патчи объявляют `public static Harmony? Harmony { get; set; }`. До этого свойство не заполнялось, и все патчи с `ContentInit` молчали, не считая строки в логе клиента.
- Не включать в профили: `accessibility.disable_flash_effect`/`disable_blurry_vision`/`disable_drunk_overlay` (игровые штрафы), `viewport.scale_render` (мыло), `duty.low_end_mode_saved` и `audio.mute_unfocused` (настройки игрока).
- `wintune.rs`: EcoQoS отключается `SetProcessInformation` после старта; GPU-предпочтение пишется в `HKCU\Software\Microsoft\DirectX\UserGpuPreferences` по полному пути exe и при выключении удаляется, только если значение ровно наше.
- `loader/Bebra.PerfPatch/`: применяется из `ContentInit`, каждая часть по своей env. Кэш иконок безопасен, пока единственный вызывающий — `StatusIconOverlay` (он сортирует список на месте, это не мешает). Если в игре появятся новые вызовы `GetStatusIcons` — перепроверить.

## Память интерфейса

- Почти вся память лаунчера приходится на WebView2, сам процесс занимает около 15 МБ. `uihost.rs` на время игры уничтожает окно с webview (`unload_on_play`) и оставляет значок в трее, а при выходе из последней игры создаёт окно заново из конфига.
- Страницы нет, поэтому `PendingActions::page_gone` снова включает буфер событий: новая страница получит `exited` и `redial` через `take_pending_actions`. Новый прямой `app.emit` без страницы теряется, такое значение страница должна читать при старте.
- Webview нельзя создавать из колбэка окна, трея или single-instance, поэтому `reopen` работает в отдельном потоке. Этот поток держит `transition` и ждёт главный, так что главный поток этот мьютекс брать не должен (выход читает `AtomicBool`).
- Выгрузка идёт через 3 с после старта игры: страница успевает сохранить настройки, а игра, упавшая сразу, оставляет ошибку на экране без мигания окна.

## Discord Rich Presence

- `Settings::discord_rpc_mode`: `vanilla` (RPC движка), `launcher` (`presence.rs`), `off`. `discord.enabled` передаётся всегда и в обе стороны: cvar сохраняется в конфиге игры.
- В режиме `launcher` с `close_on_play` статус ведёт игра: закрывшийся лаунчер не может держать соединение с Discord (`game_keeps_presence`).
- `presence.rs` работает без библиотек: кадры `op u32 LE, длина u32 LE, JSON` в `\\.\pipe\discord-ipc-N`. Один поток на игру, обновление раз в 15 с и только при изменении, `/status` раз в 30 с; закрытие пайпа очищает статус.
- ID приложения — от самой игры (`560499552273170473`, из `DiscordRichPresence.cs`), поэтому показывается «Space Station 14» и доступна картинка `logo`. Со своим ID картинки нужно загрузить в это приложение.
- Заголовок карточки — имя приложения Discord («Space Station 14»). Текстовых строк две: сервер в `details`, персонаж и сикей в `state`, под ними таймер раунда, станция и пресет в `large_text` (подсказка на иконке).
- Иконку задаёт сервер: `discord.rich_main_icon_id` (большая) и `discord.rich_second_icon_id` (маленькая), картинка должна быть в приложении игры. Лаунчер принимает только имена картинок, не адреса (`image_key`), без данных показывает `logo`.
- Имя персонажа и иконки знает только клиент: `loader/Bebra.PresencePatch/` раз в секунду берёт `MetaDataComponent.EntityName` у `LocalEntity` и эти cvar (постфикс `GameController.Update`) и заменяет файл из `BEBRA_PRESENCE_FILE` (`logs\client-*.presence`, по значению в строке). Лаунчер читает его и удаляет после игры. У призрака или другой сущности покажется её имя. Если движок изменится, патч отключится: в строке останется сикей, а иконка будет `logo`.
- Лаунчер видит только сервер, который запустил: смены сервера внутри клиента без redial в статусе нет. После перезапуска лаунчера во время игры статус не восстанавливается (`recover_sessions` его не запускает). При нескольких клиентах у каждого своё соединение, какой статус покажет Discord, не проверялось.
- Адрес сервера в статус не попадает: он может быть приватным.

## Сборка и инструменты

- Проверки: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; в `app/`: `pnpm check`, `pnpm test`. CI (`.github/workflows/ci.yml`) делает то же, плюс сборку Loader и трёх патчей (HWID, Perf, Presence) и NuGet audit в job `loader`.
- HTTP-клиент на `native-tls` (SChannel), не `rustls` (`crates/core/Cargo.toml`). В reqwest 0.13 фича `rustls-tls` переименована в `rustls` и тянет aws-lc-rs, которому на Windows нужны NASM/cmake.
- `tauri::generate_context!` требует `app/dist`: для `cargo check` сначала `pnpm build`.
- `cargo build -p bebra-app` без Tauri CLI не вшивает фронтенд: окно грузит `127.0.0.1:1420`. Вшивает `pnpm tauri build`.
- `tauri.conf.json`: `bundle.active=false`; релизные настройки — в `tauri.release.json`.
- Capabilities (`app/src-tauri/capabilities/default.json`): только `core:event:allow-listen` и `allow-unlisten`. Свои команды доступны без capability. CSP — в `tauri.conf.json`: `default-src 'self'`, `connect-src` только `ipc:` и `http://ipc.localhost`.
- Loader: `NU1902`, `NU1903`, `NU1904` — ошибки сборки (`loader/Directory.Build.props`). `SQLitePCLRaw.bundle_e_sqlite3` не ниже 2.1.14: 2.1.10 уязвим (GHSA-2m69-gcr7-jv3q), см. `loader/Directory.Packages.props`.
- Установщик: `scripts/build-release.ps1` (NSIS). Loader — self-contained.
- `installer-hooks.nsh` хранится в UTF-8 с BOM, иначе makensis прочтёт русский текст в системной кодировке. Установщик работает без повышения прав (`currentUser`), а путь прошлой установки берёт из `HKCU\Software\BebraLauncher\BebraLauncher`: ручное удаление файлов этот ключ не стирает. Без прав на папку `Delete` в деинсталляторе молчит, поэтому хуки проверяют запись до любых изменений.
- `.gitattributes`: `eol=lf`, `*.ps1`/`*.bat`/`*.nsh` — CRLF. `rustfmt.toml`: `max_width = 100`, `newline_style = "Unix"`.
- `app/package.json`: `packageManager: pnpm@10.0.0`.
- Иконка — заглушка «B» на градиенте (`app/src-tauri/icons/source.png`).

## Не проверено вживую

Последний раз отмечено как непроверенное 2026-11-03. 2026-11-07 владелец сообщил, что большую часть живых проверок выполнил; по этому сообщению список ниже не менялся и ничего не записано как новое подтверждение.

- Zip-режим загрузки, ACZ и модули (WebView): покрыты юнит-тестами, на сервере не проверены.
- Redial из игры (`--commands`).
- Установка и перезапуск при самообновлении: на настоящем релизе не проверялись.
- HWID на сервере.
- Время игры при `close_on_play` (см. B01 в `docs/IMPROVEMENTS.md`).

## Не сделано

- Трей вне игры: закрытие в трей и настройка `close_behavior` не сделаны. Значок в трее есть только пока идёт игра (`uihost.rs`).
- Подпись установщика Authenticode не делается: сертификат платный, владелец от него отказался.
- E2E-тесты.
- Полный список и приоритеты — `docs/IMPROVEMENTS.md`.
