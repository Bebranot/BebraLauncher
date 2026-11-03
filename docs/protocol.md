# Протокол SS14-лаунчера (результат Фазы 0)

Пометки: **[код]** — подтверждено исходниками официального лаунчера 0.40.1
(`Forks\space-wizard SS14 launcher`, пути относительно `SS14.Launcher\`),
**[live]** — проверено живым запросом 2026-10-03, **[док]** — есть в docs.spacestation14.com,
**[?]** — НЕ проверено, нельзя опираться без сверки.

Статус разделов: 1–8 написаны (2026-10-03). Проверено живьём: список/`/info` через хаб (1), манифест и POST-скачивание
блобов (3), движок (6.2), логин, продление и ping (7), запуск клиента с подключением (8). Не проверено живьём: модули (6.3),
zip-режим (4), ACZ (2), redial из игры (8.1). Ссылки `[код: ...]` указывают на файлы официального лаунчера в `..\Forks\`.

**Главный вывод Фазы 0:** лаунчер НЕ запускает Robust.Client напрямую. Он запускает отдельный
`SS14.Loader.exe` (.NET 10), который грузит Robust.Client.dll из zip движка и отдаёт ему контент
из `content.db`. Без Loader игру не запустить → см. раздел 8 и открытое решение в разделе 9.

---

## 1. Хаб, `/status`, `/info`

### 1.1 Базовые URL [код: ConfigConstants.cs:30-53]
Каждый «URL» на самом деле набор из основного и fallback-зеркала (`UrlFallbackSet`).

| Что | Основной | Fallback |
|---|---|---|
| Хаб | `https://hub.spacestation14.com/` | `https://hub.fallback.spacestation14.com/` |
| Auth | `https://auth.spacestation14.com/` | `https://auth.fallback.spacestation14.com/` |
| Билды движка (manifest.json, modules.json) | `https://robust-builds.cdn.spacestation14.com/` | `https://robust-builds.fallback.cdn.spacestation14.com/` |
| Данные лаунчера (info.json, assets/) | `https://launcher-data.cdn.spacestation14.com/` | `https://launcher-data.fallback.cdn.spacestation14.com/` |

Все базовые URL обязаны оканчиваться на `/` (проверяется в `HubApi.GetServers`).

**Fallback-стратегия** [код: Utility/UrlFallbackSet.cs, HappyEyeballsHttp.cs]: запрос идёт
в первый URL; если за 3 с (`AttemptDelay`) нет ответа, параллельно стартует второй;
побеждает первый успешный ответ (happy-eyeballs). Статистика успехов ведётся, но на выбор не влияет.

Лаунчер поддерживает **несколько хабов**: встроенный + пользовательские (`DataManager.Hubs`,
сортировка по `Priority`). Дубли серверов отбрасываются, побеждает хаб с более высоким приоритетом
(`ServerListCache.cs:62-122`). Для MVP нужен только встроенный.

### 1.2 Список серверов: `GET {hub}api/servers` [код: HubApi.cs:23-33] [live]
Ответ `application/json` — массив. На 2026-10-03 это 181 сервер, ~65 КБ.

```jsonc
{
  "address": "ss14s://eridani.imperialconnect.space/",   // ss14:// или ss14s://
  "statusData": {
    "name": "…",                    // string|null
    "players": 6,                   // int
    "soft_max_players": 100,        // int
    "round_start_time": "2026-10-03T08:38:01.2306510Z",  // ISO-8601 UTC, может отсутствовать
    "run_level": 1,                 // 0 PreRoundLobby, 1 InRound, 2 PostRound; может быть null
    "tags": ["lang:ru","region:eu_e"],   // string[]|null
    // ↓ есть в живом ответе, но НЕ в модели лаунчера 0.40.1 — игнорируем/опционально:
    "map": "Cluster", "preset": "Секрет", "round_id": 79337, "panic_bunker": false
  },
  "inferredTags": []                // тоже есть вживую, лаунчер 0.40.1 не читает
}
```
Таймаут запроса списка в лаунчере — 10 с (`ServerListCache.cs:51`).
Модель: `HubApi.ServerListEntry(Address, StatusData)`, `ServerApi.ServerStatus` [код: Api/ServerApi.cs:10-18].

**Для нашего парсера:** все поля `statusData` кроме `players` считать необязательными,
неизвестные поля не ломают разбор (serde без `deny_unknown_fields`).

### 1.3 Адреса серверов [код: UriHelper.cs]
- Схемы: `ss14://` (HTTP) и `ss14s://` (HTTPS). Без схемы → подставляется `ss14://`.
- Адрес HTTP-API сервера: `ss14` → `http`, `ss14s` → `https`.
- **Порт по умолчанию:** для `ss14://` без порта — **1212**; для `ss14s://` — стандартный 443.
- К пути добавляется завершающий `/`; затем `status`, `info`, `client.zip`.
- Хост пустой → адрес невалиден.

### 1.4 Статус сервера: `GET {api}/status` [код: ServerStatusCache.cs:78-120] [док] [live]
Напрямую у игрового сервера. Тот же формат, что `statusData` из 1.2.
Таймаут **5 с** (`ServerStatusTimeout`). Любая ошибка сети/JSON → сервер «Offline».
Клиент округляет `players`/`soft_max_players` вверх до 0 (`Math.Max(0, …)`).
`run_level`: InRound → «в раунде»; PostRound и PreRoundLobby → «в лобби»; иначе «неизвестно».
Живая проверка: из 6 самых загруженных серверов 5 ответили 200, один (`eridani…`) завис по таймауту
с этой машины — **таймаут/Offline штатная ситуация**, обрабатывать без паники.

### 1.4a Доп. теги [код: ServerTagInfer, Api/ServerApi.cs:30-69]
Теги — строки: `18+`, `region:<код>`, `lang:<код>`, `rp:<none|low|med|high>`, `no_tag_infer`.
Лаунчер дополнительно *выводит* теги из имени сервера (если нет `no_tag_infer`).
[?] Правила вывода (`ServerTagInfer`) ещё не прочитаны — это UI-фича, не блокер MVP.

### 1.5 Информация о сервере: `GET {api}/info` [код: ServerStatusCache.cs:232-240, Models/ServerInfo.cs] [док]
Прямой запрос: `GET {api}/info?can_skip_build=1`.
[?] Точная семантика `can_skip_build` в доках не описана; при реальном скачивании билда
(Фаза 1) запрашивать `/info` **без** этого параметра, пока не сверимся с RobustToolbox.

**Через хаб (для серверов из хаба):** `GET {hub}api/servers/info?url=<urlencoded ss14-адрес>`
[код: HubApi.cs:35-39] [live] — хаб проксирует `/info`. Работает даже когда сам сервер
недоступен с нашей машины (проверено на `eridani…`). Формат ответа тот же.

Ответ [код: Models/ServerInfo.cs] [live]:
```jsonc
{
  "connect_address": "udp://eridani.imperialconnect.space:1214/", // string|null — UDP-адрес игры
  "auth": { "mode": "Required", "public_key": "<base64 libsodium sealed-box key>" },
                                                   // mode: Optional | Required | Disabled
  "build": {                                       // может отсутствовать
    "engine_version": "277.0.0",
    "version": "20260904051054",
    "fork_id": "eridani",
    "hash": "8F4A…",                // SHA-256 всего client.zip, hex [код: Updater.Zip.cs:81-92]; zip-режим
    "download_url": "https://…/SS14.Client.zip",  // zip-режим (legacy), может быть null
    "manifest_url": "https://…/manifest",         // manifest-режим
    "manifest_hash": "EBA9…",       // BLAKE2b-256 текста манифеста, hex [live: проверено]
    "manifest_download_url": "https://…/download",
    "acz": false                    // «Auto Client Zip» — клиент берётся с самого сервера [?] детали
  },
  "desc": "…",                      // string|null
  "links": [ { "name": "Дискорд", "icon": "discord", "url": "…" } ],
  "privacy_policy": { "link": "…", "identifier": "…", "version": "…" }   // опционально
}
```
Поля `build.*` — основа Фазы 1 (выбор zip vs manifest, hash, engine). Разбор — в разделе 2.
`auth.public_key` лаунчер лишь передаёт клиенту (раздел 8); что именно с ним делает клиент — [?], в коде лаунчера не видно.

### 1.6 Что отсюда следует для архитектуры core
- `HubClient::list_servers() -> Vec<ServerEntry>`; один и тот же `UrlFallbackSet` (happy eyeballs) для хаба/auth/CDN.
- `ServerAddr::parse()` с правилами из 1.3 + `api_base()`.
- `ServerStatus` отдельно от `ServerInfo`; статусы опрашивать параллельно с лимитом (181 сервер!), таймаут 5 с.
- Для hub-серверов `/info` брать через хаб; для адресов, введённых руками (прямое подключение) — напрямую.

---

## 2. Выбор способа загрузки билда [код: Updater.cs:679-730, Connector.cs:450-485]

Вход — объект `build` из `/info` (раздел 1.5).

**Шаг 0 — ACZ / самохостинг** (Connector.cs:464-478). Если `build.acz == true` ИЛИ `download_url` пуст:
- `download_url := {api}/client.zip`;
- если `acz == true`: `manifest_url := {api}/manifest.txt`, `manifest_download_url := {api}/download`.

Здесь `{api}` — HTTP-адрес игрового сервера (раздел 1.3). Живой пример: `game.hardlight.space` отдаёт
`acz:true` и пустые `manifest_url`/`download_url` [live]. Манифестный режим при ACZ включится только
если сервер ещё и прислал `manifest_hash` (см. шаг 1), иначе пойдёт zip с `/client.zip`. [?] проверить на ACZ-сервере в Фазе 1.

**Шаг 1 — режим.** Манифестный, если **все три** поля непусты: `manifest_url`, `manifest_download_url`,
`manifest_hash`. Иначе zip, если есть `download_url`. Иначе ошибка «No download information».
Манифестный режим предпочтителен: докачивает только недостающие файлы (delta).
Живой опрос 12 самых загруженных серверов: 11 имеют манифест+zip, 1 — ACZ.

**Шаг 2 — уже скачано?** (`CheckExisting`, Updater.cs:391-454). Ищем в ContentDB версию:
1. есть `manifest_hash` → `ContentVersion.Hash == manifest_hash` (источник истины);
2. иначе есть `hash` → `ContentVersion.ZipHash == hash`;
3. иначе по `(fork_id, version)` («и надеемся на лучшее», для тестов).

Несколько кандидатов → предпочитается совпадение `ForkVersion`+`ForkId`+версии движка.
Нашли, но `fork_id`/`version`/`engine_version` у сервера отличаются → создаётся **копия** записи версии
(манифест копируется строками, а не блобами), движок пересчитывается (`DuplicateExistingVersion`).
Совпало всё → только `LastUsed = now`.

**Шаг 3 — зависимости.** После скачивания: `ContentEngineDependency` получает `('Robust', engine_version)` и,
если в контенте есть `manifest.yml`, модули из него (раздел 6.3).

**Транзакции/обрыв** (Updater.cs:482-525): загрузка идёт в одной SQLite-транзакции. При исключении, если уже
скачаны блобы, их id сохраняются в `InterruptedDownload*`, а недостроенная `ContentVersion` удаляется, транзакция
коммитится: докачка продолжится с готовых блобов. Старые «обрывки» чистятся через 7 суток.

---

## 3. Manifest-режим (delta) [код: Updater.Manifest.cs] [док: delta-updates-and-manifests]

### 3.1 Манифест: `GET {manifest_url}` [код: :88-142] [live: проверено на rmc14]
- Заголовок запроса `Accept-Encoding: zstd`. Сервер отвечает `Content-Encoding: zstd`
  (сжатый размер 2.6 МБ → 9.1 МБ текста). Без этого заголовка отдаёт plain-текст.
  Лаунчер раскрывает zstd, если *последний* `Content-Encoding` равен `zstd` (HttpUtility.cs:27).
- Ответ также содержит заголовок `X-Manifest-Hash` [live]; лаунчер его **не использует**.
- Формат (UTF-8, без BOM, `\n`):
  ```
  Robust Content Manifest 1
  <HEX64 BLAKE2b-256 файла> <путь>
  ...
  ```
  Разделитель — **первый** пробел (пути могут содержать пробелы: 18 таких в живом манифесте).
  Первая строка обязана быть ровно `Robust Content Manifest 1`, иначе ошибка.
- **Проверка:** `BLAKE2b-256` от ВСЕГО текста (включая заголовок и финальный `\n`) должен совпасть с
  `build.manifest_hash` (hex без учёта регистра). **[live: совпало: 20F419AA...15C0]**
- Наблюдения [live]: 70 827 записей, без директорий, отсортировано по пути побайтно (UTF-8), хеш 64 hex-символа.
  Один и тот же хеш может встречаться у нескольких путей (дедупликация блобов).

### 3.2 Что докачивать [код: :144-184]
Для каждой записи манифеста ищем `Content.Hash` в БД. Нет → индекс записи в список загрузки
(каждый уникальный хеш один раз, даже если встречается в манифесте несколько раз).

### 3.3 Скачивание блобов [код: :186-407, :409-439] **[live: проверено 2026-10-03 на lizard.spacestation14.com: 6 552 уникальных блоба, все BLAKE2b сошлись, манифест-хеш пересчитан из БД и совпал]**
1. **Проверка протокола:** `OPTIONS {manifest_download_url}` → заголовки `X-Robust-Download-Min-Protocol` и
   `X-Robust-Download-Max-Protocol`; наш протокол `1` обязан лежать в [min, max]. **[live: 204, min=1 max=1]**
2. `POST {manifest_download_url}`:
   - заголовки: `X-Robust-Download-Protocol: 1`, `Accept-Encoding: zstd`, `Content-Type: application/octet-stream`;
   - тело: массив **int32 little-endian**: индексы записей манифеста (в порядке манифеста), ровно 4 байта на файл.
3. Ответ (после снятия HTTP-zstd, если есть `Content-Encoding: zstd`) это поток:
   ```
   int32 LE  flags                 // bit0 = PreCompressed
   повторять для каждого запрошенного файла, в порядке запроса:
     int32 LE  uncompressedLength
     [если PreCompressed] int32 LE compressedLength   // 0 = файл НЕ сжат
     байты: если compressedLength > 0 -> compressedLength байт zstd-кадра, иначе uncompressedLength байт «как есть»
   ```
4. Для каждого файла: восстановить несжатые данные, посчитать BLAKE2b-256, сверить с хешем записи манифеста;
   несовпадение → ошибка «Hash mismatch while downloading». Размер после распаковки обязан быть равен `uncompressedLength`.
5. Хранение в БД: если поток PreCompressed и файл пришёл сжатым, кладём сжатые байты как есть (`Compression=2`).
   Если поток не PreCompressed, лаунчер сам пробует сжать zstd и сохраняет сжатое, только если экономит
   больше 10 байт (`CompressionSavingsThreshold`).
6. После загрузки: заполнение `ContentManifest(VersionId, Path, ContentId)` для всех записей.
   Все блобы обязаны быть в БД (иначе `UnreachableException`).

---

## 4. Zip-режим (legacy) [код: Updater.Zip.cs]

1. `GET {download_url}` во временный файл (прогресс по байтам). Fallback-зеркал нет, это прямой URL билда.
2. Проверка: `SHA-256` всего zip (hex, без учёта регистра) == `build.hash`, если `hash` задан; иначе просто
   запоминаем посчитанный хеш в `ZipHash`. Несовпадение → `InvalidOperationException`.
3. Загрузка в БД, для каждой записи zip (`ZipIngest`): пропуск директорий (`entry.Name == ""`);
   `BLAKE2b-256` содержимого; если такого блоба нет, вставка; сжатие zstd применяется, если
   `entry.Length - entry.CompressedLength > 10` (решение по размеру *внутри zip*); запись в `ContentManifest`.
4. Манифест-хеш версии считается вручную (раздел 5.3) и пишется в `ContentVersion.Hash`.

---

## 5. ContentDB [код: Models/ContentManagement/*, Migrations/*.sql]

### 5.1 Расположение и режим
- Файл `content.db`. Windows: `%LOCALAPPDATA%\Space Station 14\launcher\content.db` [код: LauncherPaths.cs:33-36,81-87]
  (имя `launcher` можно переопределить env `SS14_LAUNCHER_APPDATA_NAME`).
- `PRAGMA journal_mode=WAL` (чтобы докачка не блокировалась запущенной игрой).
- Строка подключения: `Mode=ReadWriteCreate;Pooling=False;Foreign Keys=True`. Без пула, чтобы WAL
  усекался сразу после закрытия (WAL может раздуваться >100 МБ).
- Миграции 0000-0004 (скрипты в репозитории). Схема ниже итоговая.

### 5.2 Схема (итог по Script0000-0004)
```sql
ContentVersion(Id INTEGER PK AUTOINCREMENT, Hash BLOB NOT NULL /*BLAKE2b манифеста*/, ForkId TEXT, ForkVersion TEXT,
               LastUsed DATE NOT NULL, ZipHash BLOB /*SHA-256 zip*/)
Content(Id INTEGER PK AUTOINCREMENT, Hash BLOB NOT NULL UNIQUE /*BLAKE2b-256 несжатых данных*/, Size INTEGER NOT NULL,
        Compression INTEGER NOT NULL /*0 None, 1 Deflate, 2 ZStd*/, Data BLOB NOT NULL,
        CHECK(Compression != 0 OR length(Data) = Size))
ContentManifest(Id INTEGER PK, VersionId -> ContentVersion ON DELETE CASCADE, Path TEXT NOT NULL,
                ContentId -> Content ON DELETE RESTRICT, CHECK(Path NOT LIKE '%/'))   -- UNIQUE(VersionId, Path); INDEX(ContentId)
ContentEngineDependency(Id PK, VersionId -> ContentVersion CASCADE, ModuleName TEXT, ModuleVersion TEXT)  -- UNIQUE(VersionId, ModuleName)
InterruptedDownload(Id PK AUTOINCREMENT, Added DATE)
InterruptedDownloadContent(Id PK AUTOINCREMENT, InterruptedDownloadId -> CASCADE, ContentId -> Content CASCADE, UNIQUE(ContentId))
RunningClient(ProcessId INTEGER PK, MainModule TEXT, UsedVersion -> ContentVersion ON DELETE RESTRICT)
```
`ModuleName='Robust'` в `ContentEngineDependency` это версия движка.

**Совместимость критична:** Loader (раздел 8) читает `Content`/`ContentManifest`/`RunningClient` напрямую
(ContentDbFileApi.cs:105-139). Если мы пишем в `content.db`, схема и семантика обязаны совпадать с оф. лаунчером.
Блобы пишутся через `zeroblob(n)` + инкрементальная запись в BLOB (SQLite incremental blob I/O), чтобы не держать файл в памяти.

### 5.3 Хеш манифеста версии, посчитанный локально [код: Updater.cs:788-824]
`BLAKE2b-256( "Robust Content Manifest 1\n" + для каждой записи по возрастанию Path: "<HEX хеша> <путь>\n" )`,
UTF-8 без BOM. Должен совпасть с `manifest_hash` сервера (в DEBUG-сборке лаунчер это ассертит).

### 5.4 Уборка [код: Updater.cs:308-389]
- Хранить не более **15** версий всего и не более **3** одного `ForkId` (`MaxVersionsToKeep`, `MaxForkVersionsToKeep`).
  Сначала удаляются давно не использованные (`ORDER BY LastUsed DESC`, лишние под нож).
- Версию, которую использует живой клиент (`RunningClient` + проверка, что процесс жив и `MainModule` совпадает), не трогать.
- Старые `InterruptedDownload` > 168 ч удаляются; затем осиротевшие `Content` без ссылок удаляются.
- «Очистить всё» запрещено, пока клиент запущен; после очистки `VACUUM`.

---

## 6. Движок и модули [код: Models/EngineManager/*]

### 6.1 Индекс билдов: `GET https://robust-builds.cdn.spacestation14.com/manifest.json` [live: 1.2 МБ]
Словарь `версия -> { date, insecure, redirect?, platforms: { <rid>: { url, sha256, sig } } }`.
- `redirect` это версия-псевдоним: идти по цепочке до записи без `redirect` (EngineManagerDynamic.Manifest.cs:86-93).
  Настоящая версия движка может отличаться от запрошенной (в `ContentLaunchInfo` пишется итоговая).
- `insecure: true` → отказ («Specified engine version is insecure!»). Много старых версий помечены insecure.
- RID-ы [live, 277.0.0]: `win-x64`, `win-arm64`, `linux-x64/arm64`, `osx-x64/arm64`, `freebsd-*`. Выбор: BFS по
  графу импортов `runtime.json`; для нас достаточно: x64 → `win-x64`, ARM64 → `win-arm64`.
- URL файлов лежат на `https://robust-builds.playss14.com/builds/<ver>/Robust.Client_<rid>.zip` [live],
  т.е. на другом хосте, чем индекс. Не хардкодить, брать `url` из манифеста.
- Манифест лаунчер кеширует 15 минут; если нужной версии нет в кеше, перезапрашивает сразу.

### 6.2 Установка движка [код: EngineManagerDynamic.cs:74-141]
Скачать zip в `%APPDATA%\Space Station 14\launcher\engines\<версия>.zip` (**Roaming**, в отличие от content.db в Local).
Запись `InstalledEngineVersion(version, signature)` хранится в БД настроек лаунчера (раздел 7.3).
**Лаунчер НЕ проверяет `sha256` при скачивании**: только запоминает `sig`; подпись проверяет Loader при каждом запуске.
Наше ядро проверяет `sha256` и Ed25519-подпись сразу после скачивания. **[live: движок 291.0.0 win-x64, 7.5 МБ — sha256 и подпись сошлись]**
Удаление: движки, на которые не ссылается ни одна версия контента (с учётом redirect), удаляются.

### 6.3 Модули (например `Robust.Client.WebView`) [код: :143-241, IEngineManager.cs:39-56]
- Нужные модули берутся из `manifest.yml` в контенте: YAML (camelCase, неизвестные поля игнорировать):
  `modules: [..]`, `multiWindow: bool`.
- Индекс: `GET https://robust-builds.cdn.spacestation14.com/modules.json` ->
  `{ "modules": { <name>: { "versions": { <ver>: { insecure, platforms: { <rid>: {url, sha256, sig} } } } } } }` [live].
- **Выбор версии модуля:** модули идентифицируются *минимальной* версией движка. Берём максимальную `ver` из
  имеющихся такую, что `ver <= engine_version` (сравнение как System.Version).
- Скачать zip, **проверить Ed25519-подпись `sig`** (hex) по всему файлу публичным ключом из `signing_key`
  (PEM SPKI, раздел 7.5); при провале ошибка; затем распаковать в `%APPDATA%\...\launcher\modules\<name>\<ver>\`.
- Каталог модуля передаётся клиенту через env `ROBUST_MODULE_<NAME>` (раздел 8).

---

## 7. Авторизация [код: Api/AuthApi.cs, Models/Logins/LoginManager.cs] (логин проверен живьём 2026-10-03, см. NOTES.md)

База: `https://auth.spacestation14.com/` (+ fallback, раздел 1.1). Тела JSON в camelCase
(`JsonSerializerDefaults.Web`, Helpers.AsJson / JsonContent.Create). [?] Точные имена полей сверить в Фазе 2 по реальному ответу.

| Действие | Запрос | Успех | Ошибка |
|---|---|---|---|
| Вход | `POST api/auth/authenticate` `{username?, userId?, password, tfaCode?}` | 200 `{token, username, userId, expireTime}` | 401 `{errors[], code}`; code: `InvalidCredentials`, `AccountUnconfirmed`, `TfaRequired`, `TfaInvalid`, `AccountLocked` |
| Продление | `POST api/auth/refresh` `{token}` | 200 `{expireTime, newToken}` | 401 = токен мёртв, нужен повторный вход |
| Проверка | `GET api/auth/ping`, заголовок `Authorization: SS14Auth <token>` | 2xx = жив | 401 = мёртв **[live: с мусорным токеном 401]** |
| Выход | `POST api/auth/logout` `{token}` | 2xx | ошибки глотаются, токен живёт до истечения |
| Регистрация/сброс | `api/auth/register`, `resetPassword`, `resendConfirmation` | | для нас **не нужны**: регистрация и управление аккаунтом на сайте (`account.spacestation14.com`) |

### 7.1 Правила токенов [код: ConfigConstants.cs:12-16, LoginManager.cs]
- `expireTime` абсолютное время. Токен истёк, если `expireTime <= now`.
- Продлевать, если до истечения **меньше 15 суток** (`TokenRefreshThreshold`); при запуске для всех аккаунтов,
  при долгом аптайме раз в **7 суток**. Запросы разносятся по времени (200 мс x индекс).
- Если продление не нужно, а статус «неизвестен», проверка через `ping`.
- Сетевая ошибка при продлении/проверке: токен мёртвым не считать (`AuthApiException`, статус остаётся).

### 7.2 Что лаунчер делает с токеном при запуске игры
Токен и параметры уходят клиенту **через переменные окружения** (раздел 8.2). Подтверждение личности при
входе на игровой сервер делает **клиент (Robust)**, лаунчер в рукопожатии не участвует. Протокол клиент-сервер-auth
в коде лаунчера отсутствует и нам не нужен (всё делает Robust.Client).
Если `auth.mode == Disabled` или аккаунт не выбран, `ROBUST_AUTH_*` не передаются, а имя берётся
из `FallbackUsername` = `JoeGenero` (гостевой вход).

### 7.3 Как хранит оф. лаунчер и чего НЕ делаем мы
Токены лежат в **открытом виде** в SQLite настроек лаунчера (`Login(UserId, UserName, Token, Expires)`,
DataManager.cs:295,455-464). Пароль после входа нигде не хранится. Наше ТЗ требует безопасного хранения:
токены кладём в Windows Credential Manager / DPAPI (`CryptProtectData`), пароль не логируем и не сохраняем,
в логах токены маскируем. Решение по реализации в Фазе 2.

### 7.4 Что передаётся в `ROBUST_AUTH_SERVER`
`ConfigConstants.AuthUrl.GetMostSuccessfulUrl()`: тот из двух auth-URL (основной/fallback), через который
было больше успешных запросов.

### 7.5 Публичный ключ подписи [код: signing_key]
Файл `signing_key` (PEM, SPKI, Ed25519, `MCowBQYDK2VwAyEApQ9mAhMLbmhQqRH7itgNo75S5rCSMsMXvVRmMv1d9NQ=`)
лежит рядом с лаунчером. Им проверяются подписи движка (в Loader) и модулей (в лаунчере).
Ключ сверен с апстримом (raw.githubusercontent.com/space-wizards/SS14.Launcher/master/SS14.Launcher/signing_key): **идентичен**; подпись реального движка им проверена.
Ядро (`crates/core/src/signature.rs`) встраивает ещё два ключа — `signing_key_multiverse.pem` и
`signing_key_supermatter.pem`; для движков с такими именами (`multiverse`, `supermatter`) Loader получает свой файл ключа
(`launch.rs`, `ensure_signing_key_file_for_engine`). Источник этих двух ключей в документации не описан.

---

## 8. Запуск клиента [код: Connector.cs:271-398, 499-621; SS14.Loader/*]

### 8.1 Loader это отдельная программа
`SS14.Loader.exe` это .NET 10 приложение из репозитория лаунчера (MIT, как и `Robust.LoaderApi`).
Что оно делает (Loader/Program.cs):
1. Читает `signing_key`, проверяет Ed25519-подпись **всего** engine-zip (`sig` из 6.1); провал → код выхода 2.
   В нашей версии Loader выключателя подписи нет (см. `loader/README.md`).
2. Открывает engine-zip, находит `Robust.Client.dll`, берёт `LoaderEntryPointAttribute`, создаёт точку входа.
3. Монтирует контент из `content.db` (`ContentDbFileApi`) и вызывает `Main` движка с нашими аргументами.
4. `RedialApi`: для «перезахода» клиент перезапускает **лаунчер**: `<launcher> --commands :RedialWait R<hex причины> C<hex адреса>`
   (путь берётся из env `SS14_LAUNCHER_PATH`). [код: LauncherCommands.cs, LauncherMessaging.cs]
   Команды: `:Ping` (показать окно), `:RedialWait` (подождать), `R<hex>`/`r<текст>` (причина), `C<hex>`/`c<адрес>` (подключиться).
   Реализовано у нас (app/src-tauri/src/redial.rs): вместо named pipe используется плагин single-instance (второй процесс
   передаёт argv первому), `SS14_LAUNCHER_PATH` ставит `core::launch` (наш exe). Вживую с настоящим redial НЕ проверено,
   разбор команд покрыт тестами. Ссылка `ss14://…` одним аргументом только открывает сервер (оф. лаунчер подключается сразу).

### 8.2 Запуск процесса
```
SS14.Loader.exe <путь к engine zip> <sig hex> <путь к signing_key> [аргументы движка...]
```
Аргументы движка (порядок как в коде):
```
--username <имя аккаунта | JoeGenero>
--cvar display.compat=<True|False>        # режим GLES2; True если включён «compat» в настройках
--cvar launch.launcher=true
--launcher --connect-address <udp://...>  # connect_address из /info или udp://{host}:{port} из адреса сервера
--ss14-address <ss14[s]://...>            # исходный адрес сервера
--cvar build.download_url=...   build.manifest_url=...   build.manifest_download_url=...
--cvar build.version=...        build.fork_id=...        build.hash=...
--cvar build.manifest_hash=...  build.engine_version=...  # передаются только непустые;
                                                          # без engine_version/manifest_hash клиент НЕ подключится на новых CDN
```
`--launcher` подразумевает `--connect` (клиент сразу идёт на сервер, без главного меню).

Переменные окружения [код: :316-326, :520-563]:

| Переменная | Значение |
|---|---|
| `ROBUST_AUTH_TOKEN` / `_USERID` / `_PUBKEY` / `_SERVER` | токен, GUID аккаунта, `auth.public_key` сервера, самый успешный auth-URL. Только если `auth.mode != Disabled` и аккаунт выбран |
| `SS14_LOADER_CONTENT_DB` | путь к `content.db` |
| `SS14_LOADER_CONTENT_VERSION` | `ContentVersion.Id` |
| `SS14_LOADER_OVERLAY_ZIP` | опционально, zip-«подложка»; у нас — текстовые пакеты (`core::packs`) |
| `ROBUST_MODULE_<NAME>` | каталог модуля (`.` -> `_`, верхний регистр), напр. `ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW` |
| `SS14_LAUNCHER_PATH` | путь к exe лаунчера (для redial) |
| `DOTNET_TieredPGO=1`, `DOTNET_ReadyToRun=0`, `DOTNET_MULTILEVEL_LOOKUP=0` | настройки .NET. `DOTNET_gcServer=1` только для спец. контента. **`DOTNET_ROLL_FORWARD` НЕ ставить** (комментарий в коде: Loader всегда на runtime лаунчера) |
| `SS14_DISABLE_SIGNING` | не передаётся: `core::launch` удаляет эту переменную из env процесса |

`UseShellExecute=false`; stdout/stderr клиента пишутся в `logs\client.stdout.log` и `client.stderr.log`
(`FileShare.ReadWrite|Delete`). Лаунчер ждёт 300 мс: если клиент умер с кодом != 0, это «клиент упал сразу».
Токен передаётся в env: в логи команду запуска писать **без значений env** (оф. лаунчер логирует только аргументы).

### 8.3 Нужный .NET [код: Launcher.props, download_net_runtime.py, Bootstrap/Program.cs, publish.py]
- TFM `net10.0`, рантайм **.NET 10.0.0** (`dotnet-runtime-10.0.0-win-x64.zip` с `builds.dotnet.microsoft.com`).
- Оф. релиз кладёт рядом: `bin_x64\` (лаунчер), `bin_x64\loader\` (Loader), `dotnet_x64\` (рантайм); Bootstrap-exe
  выставляет `DOTNET_ROOT=dotnet_x64` и стартует лаунчер. Loader наследует `DOTNET_ROOT` и берёт рантайм оттуда.
- Нативные зависимости Loader: SQLite (e_sqlite3), libsodium, Robust.Natives(.Zstd); приходят с publish.
- VC++ Redistributable x64 нужен на Windows (в лаунчере есть проверка `VcRedistCheck.cs`). [?] детали позже.

---

## 9. Выводы для нашей архитектуры и принятое решение

**Core (Rust) реализуем сами:** хаб/`/status`/`/info`, выбор режима, zip- и manifest-загрузка, ContentDB (схема 5.2),
индекс/скачивание движка и модулей + Ed25519-проверки, авторизация (token store), сборка команды запуска и env.

**Loader нельзя написать на Rust**: он хостит управляемый Robust.Client.dll в .NET-процессе.
Принято вариант A: `SS14.Loader` собирается из исходников официального лаунчера (MIT, каталог `loader/`) через `dotnet publish`
и поставляется с установщиком вместе с .NET 10 runtime (см. `loader/README.md`). Варианты B (скачивать Loader у оф. лаунчера)
и C (переписать на .NET) отклонены.

Вес: в установщике Loader self-contained (~96 МБ в ресурсах по NOTES.md). [?] Оценка .NET runtime + Loader для «малого веса» не замерялась.

**Прочее для Фазы 1:** схему `content.db` не менять; тесты хеширования (BLAKE2b-256 без ключа = `blake2b(digest_size=32)`),
парсер манифеста, парсер `/info`; скачивание блобов сначала на малом наборе файлов, с явного согласия пользователя.
