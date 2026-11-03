# Протокол SS14-лаунчера

Пометки: **[код]** — исходники официального лаунчера 0.40.1 (`Forks\space-wizard SS14 launcher`, пути относительно `SS14.Launcher\`);
**[live]** — живой запрос (дата в скобках); **[док]** — docs.spacestation14.com; **[?]** — не проверено, опираться нельзя.
**(не реализовано в лаунчере)** — поведение upstream, которого нет в `crates/` и `app/` (проверено поиском).
Наш код: `crates/core/src`, `loader/`.

Статус: разделы 1–8 сверены с кодом 2026-11-03. Живые проверки не повторялись, даты в пометках.

**Главный вывод:** лаунчер НЕ запускает Robust.Client напрямую. Он запускает `SS14.Loader.exe` (.NET 10), который грузит
Robust.Client.dll из zip движка и отдаёт ему контент из `content.db`. См. раздел 8 и решение в разделе 9.

---

## 1. Хаб, `/status`, `/info`

### 1.1 Базовые URL и сеть [код: ConfigConstants.cs:30-53; наш код: `hub.rs`, `auth.rs`, `engine.rs`, `http.rs`]

| Что | Основной | Fallback |
|---|---|---|
| Хаб | `https://hub.spacestation14.com/` | `https://hub.fallback.spacestation14.com/` |
| Auth | `https://auth.spacestation14.com/` | `https://auth.fallback.spacestation14.com/` |
| Билды движка (manifest.json, modules.json) | `https://robust-builds.cdn.spacestation14.com/` | `https://robust-builds.fallback.cdn.spacestation14.com/` |
| Данные лаунчера (info.json, assets/) | `https://launcher-data.cdn.spacestation14.com/` | `…fallback…` (**не реализовано в лаунчере**) |

Базовые URL нормализуются к завершающему `/`.

- **`UrlFallbackSet.send`** (GET): старт с первого зеркала; через 3 с (`ATTEMPT_DELAY`) или сразу при ошибке транспорта
  стартует следующее. Побеждает первый ответ **с любым статусом** (5xx не переключает зеркало). Upstream «первый успешный» — не так.
- **`send_ordered`** (POST login/refresh/logout): зеркала по очереди, без параллели. Дальше — при ошибке транспорта или статусе
  вне `accept` (login/refresh: 2xx и 401; logout: 2xx).
- `most_successful`: считаются только 2xx. Используется лишь для `ROBUST_AUTH_SERVER` (7.4).
- Сеть: connect timeout 10 с; User-Agent `BebraLauncher/<версия>`; прокси `http(s)://`, `socks5(h)://` (в ошибках без userinfo).

**Несколько хабов** (`hub::list_servers_from_hubs`): параллельный опрос. URL хаба (`is_valid_hub_url`): `https://` или `http://`
только для `localhost`/`127.0.0.1`/`[::1]`; без userinfo, query, fragment; ≤ 300 символов.
Дубли по `address` сливаются: `players` — максимум, теги объединяются, пустые `name`/`map`/`preset` дополняются.
Сортировка: `players` убыв., затем `address`. Ошибка только если все хабы упали.
Upstream отбрасывает дубли по приоритету хаба (`ServerListCache.cs:62-122`); приоритетов у нас нет.

### 1.2 Список серверов: `GET {hub}api/servers` [код: HubApi.cs:23-33] [live: 2026-10-03]
Ответ — JSON-массив (181 сервер, ~65 КБ).

```jsonc
{
  "address": "ss14s://eridani.imperialconnect.space/",   // ключ сервера в хабе: строка как есть
  "statusData": {                                         // null допустим (сервер не отчитался)
    "name": "…", "players": 6, "soft_max_players": 100,
    "round_start_time": "2026-10-03T08:38:01.2306510Z",   // ISO-8601 UTC, может отсутствовать
    "run_level": 1,                                       // см. 1.4
    "tags": ["lang:ru","region:eu_e"],                    // null → []
    "map": "Cluster", "preset": "Секрет", "round_id": 79337, "panic_bunker": false, "auth_methods": null  // опциональны
  },
  "inferredTags": []                                      // null → []
}
```
- Таймаут 10 с на запрос (`LIST_TIMEOUT`, [код: ServerListCache.cs:51]).
- `429`: ждать `Retry-After` (секунды; нет или мусор — 1 с), не дольше 10 с, максимум 2 повтора, затем ошибка.
- Тело ≤ 8 MiB; записей учитывается ≤ 5000.
- Запись, которую не разобрать, пропускается и считается в `skipped`; остальные показываются.
- Все поля `statusData` опциональны (`players` по умолчанию 0); неизвестные поля игнорируются.
- Типы: `HubServerEntry` (`hub.rs`), `ServerStatus` (`server.rs`) [код: Api/ServerApi.cs:10-18].

### 1.3 Адреса серверов [код: UriHelper.cs; наш код: `addr.rs` `ServerAddr::parse`]
- Схемы: `ss14://` (HTTP), `ss14s://` (HTTPS), синонимы `http://`/`https://`. Без схемы — `ss14://`. Схема регистронезависима.
- Отвергаются: пустая строка, управляющие символы, userinfo (`@`), `?`, `#`, пробелы в хосте/пути, порт `0` или нечисловой,
  IPv6 без скобок.
- Хост: lower-case, IDNA, IPv4, IPv6 в `[...]`. `SS14://EXAMPLE.COM` и `ss14://example.com` — один сервер; хаб ключуется исходной строкой.
- **Порт по умолчанию:** `ss14://` без порта → **1212** (API и UDP); `ss14s://` без порта → 443.
- API-база: `http(s)://host[:port]/path/`. От неё: `status`, `info`, `client.zip`, а для ACZ — `manifest.txt`, `download`.
- Адрес для клиента (`--ss14-address`): `ss14[s]://host[:port]/path`, `/` если пути нет; порт только явный.
- UDP по умолчанию (`--connect-address`): `udp://host:port/`, порт = явный, иначе 1212 (`ss14`) / 443 (`ss14s`).

### 1.4 Статус: `GET {api}/status` [код: ServerStatusCache.cs:78-120; наш код: `server.rs` `fetch_status`] [док] [live]
- Формат — как `statusData` (1.2). Таймаут **5 с** (`STATUS_TIMEOUT`), тело ≤ 4 MiB.
- Не-2xx, сетевая ошибка, битый JSON — ошибка (UI показывает «offline»).
- `players`, `soft_max_players`: int, float (`12.0`) или строка (`"12"`). Отрицательные и нечисловые → 0.
- `run_level`: `0|1|2` или имя: `PreRoundLobby`/`lobby` → 0, `InRound`/`in_round` → 1, `PostRound` → 2 (регистр не важен, `_`/`-` игнорируются). Прочее → `None`.
- Таймаут — штатная ситуация [live: 2026-10-03, один из 6 загруженных серверов завис].

### 1.4a Теги [код: upstream ServerTagInfer; наш код: `server.rs` `infer_tags`, `all_tags`]
Теги: `18+`, `region:<код>`, `lang:<код>`, `rp:<none|low|med|high>`, `no_tag_infer`.
Вывод из имени (регистр не важен; нет `no_tag_infer`; применяется при слиянии списка хаба):
- `lang:ru` при `[ru]`, `[rus]`, `[ру]`, ` ru `; иначе `lang:en` при `[en]`, `[eng]`, ` en ` — только если нет `lang:*`;
- `18+` при `18+`, `+18`, `[erp]` — если нет `18+`;
- `rp:high` (`[hrp]`, `[hrp+]`, ` hrp `) > `rp:med` (`mrp`) > `rp:low` (`lrp`) > `rp:none` (`nrp`) — если нет `rp:*`.

### 1.5 Информация: `GET {api}/info` [код: ServerStatusCache.cs:232-240; наш код: `server.rs`] [док]
- **Перед подключением** (`fetch_info`): `GET {api}info`, без query, таймаут **15 с** (`INFO_TIMEOUT`); применяется самохостинг (раздел 2, шаг 0).
- **Только показ** (`fetch_info_preview`): `GET {api}info?can_skip_build=1`, таймаут 5 с, `build` может отсутствовать, без самохостинга. Семантика `can_skip_build` не описана в доке [?].
- **Через хаб** (`HubClient::server_info`): `GET {hub}api/servers/info?url=<urlencoded address>`, таймаут 5 с [код: HubApi.cs:35-39] [live].
  Хаб проксирует `/info`, работает, даже если сервер недоступен напрямую (проверено на `eridani…`). Самохостинг — в `play::prepare`.
- Тело ≤ 4 MiB.

```jsonc
{
  "connect_address": "udp://eridani.imperialconnect.space:1214/",   // см. правила ниже
  "auth": { "mode": "Required", "public_key": "<base64 libsodium sealed-box key>" },
           // mode: Optional | Required | Disabled, регистр не важен; иное → Unknown (как Optional). Нет auth → Optional
  "build": {                                 // может отсутствовать; пустые строки = отсутствует
    "engine_version": "277.0.0", "version": "20260904051054", "fork_id": "eridani",
    "engine_type": "Robust",                 // null/пусто → "Robust"; также multiverse, supermatter (раздел 6)
    "hash": "8F4A…",                         // SHA-256 всего client.zip, hex [код: Updater.Zip.cs:81-92]
    "download_url": "…/SS14.Client.zip",     // zip-режим (legacy), может быть null
    "manifest_url": "…/manifest",            // manifest-режим
    "manifest_hash": "EBA9…",                // BLAKE2b-256 текста манифеста, hex [live]
    "manifest_download_url": "…/download",
    "acz": false                             // Auto Client Zip: клиент берётся с самого сервера
  },
  "desc": "…",                               // string|null
  "links": [ { "name": "Дискорд", "icon": "discord", "url": "…" } ],   // null → []
  "privacy_policy": { "link": "…", "identifier": "…", "version": "…" }  // опционально
}
```
- `connect_address` [код: addr.rs `normalize_connect_address`]: только `udp://host:port[/]` с явным ненулевым портом, без userinfo
  и пробелов; нормализуется в `udp://host:port/`. Иначе игнорируется (берётся адрес по умолчанию, 1.3).
- `privacy_policy`: до показа ссылки и согласия подключаться нельзя. Согласие — пара `(identifier, version)`, смена версии требует
  согласия заново (`is_accepted`); хранит приложение.
- `auth.public_key` лаунчер только передаёт клиенту (`ROBUST_AUTH_PUBKEY`, 8.2). Что с ним делает клиент — [?].

### 1.6 Что это значит для core
- `HubClient::list_servers` / `list_servers_from_hubs`; `ServerAddr::parse` + `api_base()`.
- `/info` для hub-серверов — через хаб (`play::request_info`), для адресов, введённых вручную, — напрямую.

---

## 2. Выбор способа загрузки билда [код: updater.rs, play.rs, server.rs]

Вход — `build` из `/info` (1.5).

**Шаг 0 — ACZ / самохостинг** (`BuildInfo::apply_self_hosting`) [код: Connector.cs:464-478]. Если `acz == true` или `download_url` пуст:
- `download_url := {api}client.zip`;
- если `acz == true`: `manifest_url := {api}manifest.txt`, `manifest_download_url := {api}download`.

Применяется в `fetch_info` и в `play::prepare` (через хаб). Пример [live]: `game.hardlight.space` — `acz:true`, пустые `manifest_url`/`download_url`.
Манифест-режим при ACZ — только если сервер прислал `manifest_hash` (шаг 1), иначе zip с `/client.zip`. [?] ACZ с `manifest_hash` не проверен.

**Шаг 1 — режим** (`download_mode`). Манифест, если непусты **все три**: `manifest_url`, `manifest_download_url`, `manifest_hash`.
Иначе zip, если есть `download_url`. Иначе ошибка «server gave no download information at all».
Манифест предпочтителен (delta). [live: 11 из 12 популярных серверов — манифест+zip, 1 — ACZ.]

**Шаг 2 — уже есть?** (`content_db::find_version`, `updater::install_content`). Поиск `ContentVersion`:
1. `manifest_hash` → `Hash` (источник истины); иначе
2. `hash` → `ZipHash`; иначе
3. `(fork_id, version)` — только если хешей нет (для тестов).

Кандидатов несколько: сначала совпавший по `ForkVersion`+`ForkId`+версии движка, затем `LastUsed DESC, Id DESC`.
- Совпало всё → `touch_version` (`LastUsed = now`), загрузки нет.
- Отличаются fork/version/engine → `duplicate_version`: новая строка `ContentVersion` с теми же хешами и `ContentManifest`
  (блобы не копируются), `Robust` := `engine_version`. Только если модулей нет или движок совпадает; иначе — обычная установка.

**Шаг 3 — зависимости.** Манифест-режим: `ContentEngineDependency('Robust', engine_version)` пишется после проверки манифеста.
`manifest.yml` разбирается в `prepare_launch`: модули получают версии и ставятся (6.3), затем движок (6.2).
Порядок `play::prepare`: `/info` → аккаунт и место (< 500 МиБ свободно → `DiskFull`) → движок качается параллельно, если не установлен
→ `install_content` → `cleanup` (5.4) → `prepare_launch` → команда запуска (8.2).

**Прерывание и докачка:** `InterruptedDownload` создаётся до загрузки блобов. Блобы коммитятся пачками (≤ 200 файлов или ≤ 32 МиБ)
с `InterruptedDownloadContent` и heartbeat (`Added`). Строка `ContentVersion` — **только в конце**, одной транзакцией, после проверки
хеша манифеста (upstream создаёт версию раньше и удаляет недостроенную). Повтор не запрашивает блобы, уже лежащие в БД.
Zip: пачки ≤ 64 файла / ≤ 32 МиБ; версия — в конце. Осиротевшие строки старше 168 ч — см. 5.4.

---

## 3. Manifest-режим (delta) [код: Updater.Manifest.cs; наш код: `download.rs`, `manifest.rs`] [док: delta-updates-and-manifests]

### 3.1 Манифест: `GET {manifest_url}` [код: :88-142] [live: rmc14]
- Запрос: `Accept-Encoding: zstd`, таймаут 120 с. Если **последний** `Content-Encoding` = `zstd` — распаковка; иначе plain.
  Лимиты: сжатое ≤ 256 МиБ, текст ≤ 512 МиБ. [live: 2.6 МБ сжатых → 9.1 МБ текста.]
- Заголовок `X-Manifest-Hash` [live] не читается.
- Формат (UTF-8, без BOM; `\r\n` терпим):
  ```
  Robust Content Manifest 1
  <HEX64 BLAKE2b-256 файла> <путь>
  ```
  - первая строка — ровно `Robust Content Manifest 1`;
  - разделитель — **первый** пробел (в путях бывают пробелы; live: 18 таких);
  - отвергаются: пустой путь, `/` в конце, абсолютный путь, `\`, `:`, управляющие символы, сегменты `""`/`.`/`..`, дубликаты путей.
- **Проверка:** BLAKE2b-256 всего текста (с заголовком и финальным `\n`) == `build.manifest_hash` (hex, любой регистр). [live: совпало, 20F419AA…15C0]
- [live]: 70 827 записей, без директорий, отсортированы по пути побайтно; один хеш может быть у нескольких путей.

### 3.2 Что докачивать [код: :144-184; наш код: `missing_entries`]
Для каждой записи ищется `Content.Hash` в БД. Нет → в список загрузки. Каждый уникальный хеш — один раз, по первому вхождению.

### 3.3 Скачивание блобов [код: :186-407, :409-439; наш код: `download_blobs`, `read_blob_stream`] [live: 2026-10-03, lizard.spacestation14.com: 6 552 блоба, все BLAKE2b сошлись, манифест-хеш пересчитан из БД и совпал]
Таймауты: `OPTIONS` 120 с; `POST` — без общего таймаута, только idle 45 с между чтениями.

1. **Протокол:** `OPTIONS {manifest_download_url}` → 2xx, заголовки `X-Robust-Download-Min-Protocol` и `X-Robust-Download-Max-Protocol`. Протокол `1` обязан лежать в [min, max]. [live: 204, min=1 max=1]
2. **Запрос:** `POST {manifest_download_url}`, заголовки `X-Robust-Download-Protocol: 1`, `Accept-Encoding: zstd`,
   `Content-Type: application/octet-stream`; тело — int32 LE индексов записей манифеста (4 байта на файл), только недостающие.
3. **Ответ** (после снятия HTTP-zstd; несколько zstd-кадров подряд допустимы):
   ```
   int32 LE  flags                 // bit0 = PreCompressed
   для каждого запрошенного файла, в порядке запроса:
     int32 LE  uncompressedLength  // < 0 → ошибка; ≤ 512 МиБ на файл, ≤ 4 ГиБ на поток
     [если PreCompressed] int32 LE compressedLength   // 0 = файл НЕ сжат
     байты: compressedLength > 0 → zstd-кадр такой длины; иначе uncompressedLength байт как есть
   ```
4. **Проверка файла:** распаковка с контролем размера (ровно `uncompressedLength`); BLAKE2b-256 результата == хеш записи манифеста.
   Несовпадение → `HashMismatch`, файл не сохраняется.
5. **Хранение** (`PreparedBlob`): файл пришёл сжатым (`PreCompressed` и `compressedLength > 0`) → байты как есть, `Compression = 2`.
   Иначе (plain, в том числе внутри `PreCompressed`-потока) → лаунчер сжимает сам zstd уровня 1, берёт сжатое только если экономит
   больше 10 байт (`COMPRESSION_SAVINGS_THRESHOLD`), иначе `Compression = 0`. Пустой файл не сжимается.
6. **После загрузки:** создаётся `ContentVersion` (2, шаг 2), `ContentManifest(VersionId, Path, ContentId)` заполняется для всех записей;
   блоб, которого нет в БД, — ошибка (`missing blob for <path>`; upstream — `UnreachableException`). `compute_manifest_hash` (5.3)
   должен совпасть с хешем манифеста, иначе транзакция откатывается.

---

## 4. Zip-режим (legacy) [код: Updater.Zip.cs; наш код: `zipmode.rs`]

1. `GET {download_url}` в `*.zip.part` (scratch). Зеркал нет. Idle 45 с, лимит 2 ГиБ, прогресс по байтам.
2. **Проверка:** SHA-256 всего zip (hex) == `build.hash`, если `hash` задан; иначе `HashMismatch`, ничего не сохраняется.
   SHA-256 всегда пишется в `ContentVersion.ZipHash`.
3. **Разбор** (`ingest_zip`): директории пропускаются (имя пустое или атрибут директории Unix/DOS; имя с `/` в конце без атрибутов — тоже).
   Пути — по правилам 3.1; дубликаты — ошибка. Содержимое: BLAKE2b-256; уже существующий блоб не вставляется.
   Сжатие: по расширению уже сжатых форматов (`png`, `jpg`, `webp`, `ogg`, `mp3`, `mp4`, `webm`, `zip`, `woff2` и др.) — `Compression = 0`;
   иначе zstd уровня 1 по 3.3 шаг 5. Upstream решает по размеру внутри zip (`Length - CompressedLength > 10`).
   Лимиты: файл ≤ 2 ГиБ, всего ≤ 4 ГиБ.
4. **Версия** — одной транзакцией: `ContentVersion` (`Hash` = manifest-хеш по 5.3; `ZipHash` = SHA-256 zip), `ContentManifest`, `Robust`.

---

## 5. ContentDB [код: Models/ContentManagement/*, Migrations/*.sql; наш код: `content_db/`, `paths.rs`]

### 5.1 Расположение и режим
- `<root>\content.db`, `<root>` = `%LOCALAPPDATA%\BebraLauncher` [код: `paths.rs` `DataDirs`]. Портативно: рядом с exe есть `portable.txt` или `.portable` → `<папка exe>\data`.
- Upstream `%LOCALAPPDATA%\Space Station 14\launcher\content.db` и `SS14_LAUNCHER_APPDATA_NAME` — **не реализовано в лаунчере**.
- Соединение (`rusqlite`): `journal_mode=WAL` (докачка не блокируется игрой), `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5 с`,
  `cache_size=-64000`, `temp_store=MEMORY`, `mmap_size=256 МиБ`. Upstream-строка `Mode=ReadWriteCreate;Pooling=False;Foreign Keys=True` не используется.
  После загрузки: `wal_checkpoint(PASSIVE)`. После очистки с удалениями: `wal_checkpoint(TRUNCATE)` и `VACUUM` (best effort).
- Миграции `0000_Initial` … `0004_RunningClient` (`MIGRATIONS`, журнал `SchemaVersions`). `0000` ставит `PRAGMA user_version = 1` (наша метка).
  `0001_Blake2B` стирает таблицы: выполняется только для БД с меткой или пустой; иначе ошибка с инструкцией переместить файл.

### 5.2 Схема (итог миграций 0000-0004)
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
- `ModuleName='Robust'` — версия движка.
- Код пишет `Compression` 0 и 2 (1 Deflate читает Loader).
- Блоб пишется целиком параметром `INSERT`. `zeroblob(32)` — только временный `Hash` в `create_version` (затем `set_version_hash`). Лимит файла 512 МиБ.

**Совместимость критична:** Loader читает `Content`/`ContentManifest`/`RunningClient` напрямую (`ContentDbFileApi`). Схему и смысл `Compression` не менять.

### 5.3 Хеш манифеста версии [код: Updater.cs:788-824; наш код: `manifest::compute_hash`]
`BLAKE2b-256( "Robust Content Manifest 1\n" + для каждой записи по возрастанию Path (побайтно, UTF-8): "<HEX> <путь>\n" )`, UTF-8 без BOM.
Для манифест-версий сверяется с `manifest_hash` сервера (3.3 шаг 6); для zip-версий становится `ContentVersion.Hash` (4 шаг 4).

### 5.4 Уборка [код: Updater.cs:308-389; наш код: `cull.rs`, `updater::cleanup`/`cleanup_all`, `storage.rs`]
- **Политика** (`cleanup`, после установки в `play::prepare`): не более **15** версий всего и **3** на `ForkId`; остаются самые недавние (`LastUsed DESC, Id DESC`).
- **Защищённые:** `in_use` и версии из `RunningClient`. Строка `RunningClient` проверяется по pid и `MainModule`: процесс жив или
  не проверен — строка остаётся; процесса нет — строка удаляется. Ошибка FK при удалении версии = «используется».
- Затем: `InterruptedDownload` без heartbeat дольше 168 ч удаляются (минимум 10 мин). Если что-то удалено — удаляются `Content`
  без ссылок из `ContentManifest` и `InterruptedDownloadContent`.
- **Полная очистка** (`cleanup_all`, лимиты 0): всё, кроме версий живых клиентов. Запрета «пока клиент запущен» нет (upstream — не реализовано).
- Движки и модули — отдельно (`storage::clean`/`clean_all`, 6.2, 6.3).

---

## 6. Движок и модули [код: Models/EngineManager/*; наш код: `engine.rs`, `modules.rs`]

### 6.1 Индекс: `GET {builds}manifest.json` [live: 1.2 МБ]
- База — `robust-builds` + fallback (1.1). `engine_type` `multiverse` → `https://cdn.spacestationmultiverse.com/` (при ошибке `ssmv-engine-manifest`);
  `supermatter` → `https://cdn.simplestation.org/supermatter/`. У них свои ключи (7.5).
- Словарь `версия → { insecure?, redirect?, platforms: { <rid>: { url, sha256, sig } } }`.
- `redirect` — псевдоним: идти по цепочке до записи без `redirect` (≤ 32 переходов). Итоговая версия может отличаться от запрошенной.
- `insecure: true` → ошибка «marked insecure».
- RID: `win-x64`; на ARM64 сначала `win-arm64`, затем `win-x64`. Прочие RID (`linux-*`, `osx-*`, `freebsd-*`) не используются.
- URL файлов берётся из индекса [live: `robust-builds.playss14.com/builds/<ver>/Robust.Client_<rid>.zip`], хостом может отличаться. Не хардкодить.
- Кеша нет (upstream 15 мин — **не реализовано в лаунчере**): индекс запрашивается при каждой установке.
- Чтение индекса (и `modules.json`, 6.3): не больше 32 МиБ, idle 30 с.

### 6.2 Установка движка [код: EngineManagerDynamic.cs:74-141; наш код: `engine::install_engine`]
- Файлы: `<root>\engines\<version>.zip` и `<version>.sig`; `version` — **запрошенная** (из `/info`, до redirect): её же хранит
  зависимость `Robust` в БД, по ней работает очистка. `[A-Za-z0-9._+-]`, ≤ 100, не с точки.
  Upstream — `%APPDATA%\…\engines\` и `InstalledEngineVersion` в БД настроек: у нас этого нет.
- **Переиспользование:** оба файла есть, `.sig` == `sig` из индекса (без учёта регистра), SHA-256 zip == `sha256` → загрузки нет.
- **Установка:** скачивание в `<version>.zip.<pid>-<nanos>.part` (idle 45 с, лимит 2 ГиБ) → SHA-256 == `sha256` →
  Ed25519 `sig` (hex, 64 байта) по всему файлу (`signature::verify`) → атомарная замена zip → `.sig` последним (маркер завершения).
- Ошибка SHA-256 или подписи → `.part` удаляется.
- Upstream не проверяет `sha256` при скачивании (Loader проверяет подпись при каждом запуске). Мы проверяем сразу.
  [live: движок 291.0.0 win-x64, 7.5 МБ — sha256 и подпись сошлись.]
- **Удаление** (`storage::clean`): `*.zip`/`*.sig`, чья версия не числится в `Robust`-зависимостях ни одной версии, удаляются; `*.part` — всегда.

### 6.3 Модули [код: Models/EngineManager/*, IEngineManager.cs:39-56; наш код: `modules.rs`]
- Список — из `manifest.yml` контента: YAML, camelCase, неизвестные ключи игнорируются, всё опционально: `modules: [имя, …]`, `multiWindow: bool`.
  Пустой файл — «ничего особого». Модуль `Robust` (без учёта регистра) — ошибка.
- Индекс: `GET {builds}modules.json` → `{ "modules": { <name>: { "versions": { <ver>: { insecure?, platforms: { <rid>: {url, sha256, sig} } } } } } }`
  [live]. Нужен, если есть модули, которых ещё нет в зависимостях версии.
- **Выбор версии** (`version_for_engine`): модули именуются *минимальной* версией движка → берётся максимальная `ver <= engine_version`
  (числовое сравнение, суффикс `-fix-physics` отбрасывается), только не-insecure с платформой `rid`. Равные по числам — по исходной строке.
- Версия пишется в `ContentEngineDependency(module, ver)`; первая запись побеждает.
- **Установка:** если `<dir>\.installed` (JSON `{sha256, files, bytes}`) совпадает с индексом и с фактическими числом и размером файлов — готово.
  Иначе: скачать, проверить SHA-256 и Ed25519 `sig` **до** распаковки, распаковать в `<ver>.tmp-<pid>-<nanos>`, `.installed` записать последним, переименовать.
  Версии нет в индексе, но папка цела — запуск разрешён.
- Путь: `<root>\modules\<name>\<version>\`; клиенту — `ROBUST_MODULE_<NAME>` (8.2). Живьём не проверено.

---

## 7. Авторизация [код: Api/AuthApi.cs, Models/Logins/LoginManager.cs; наш код: `auth.rs`, `accounts.rs`] (логин проверен живьём 2026-10-03, см. NOTES.md)

База: `https://auth.spacestation14.com/` + fallback (1.1). JSON в camelCase, `null`-поля отправляются. Таймаут 30 с. Ответ ≤ 64 КиБ.
Поля ответа читаются и в camelCase, и в PascalCase.

| Действие | Запрос | Успех | Отказ / ошибка |
|---|---|---|---|
| Вход | `POST api/auth/authenticate` `{username \| userId, password, tfaCode?}` (второе поле `null`) | 2xx `{token, username, userId, expireTime}` | 401 `{errors[], code}` → `Denied`; тело не читается → `code = Unknown`; прочее → ошибка |
| Продление | `POST api/auth/refresh` `{token}` | 2xx `{newToken, expireTime}` | 401 → `None` (нужен вход); прочее → ошибка |
| Проверка | `GET api/auth/ping`, `Authorization: SS14Auth <token>` | 2xx → жив | 401 → мёртв [live: мусорный токен → 401]; прочее → ошибка |
| Выход | `POST api/auth/logout` `{token}` | 2xx | не 2xx → ошибка; локальный аккаунт всё равно удаляется |
| Регистрация/сброс/письмо | `register`, `resetPassword`, `resendConfirmation` | — | **не реализовано в лаунчере** (регистрация на сайте) |

`code` (регистр не важен): `None`, `InvalidCredentials`, `AccountUnconfirmed`, `TfaRequired` (запросить 2FA-код и повторить), `TfaInvalid`, `AccountLocked`; иное → `Unknown`.
Транспорт: login/refresh/logout — `send_ordered`, ping — `send` (1.1).

### 7.1 Правила токенов [код: LoginManager.cs; наш код: `accounts::freshness`, `ensure_fresh`]
- `expireTime` — абсолютное время (RFC 3339). Истёк при `expireTime <= now` → вход без сети.
- Продлевать, если до истечения **≤ 15 суток** (`REFRESH_THRESHOLD`). Новый токен, который уже истёк, считается истёкшим. Не удалось сохранить — ошибка «войти заново».
- Иначе при `verify` — `ping`; 401 → нужен вход.
- Сетевая ошибка при продлении или проверке — ошибка, аккаунт не меняется.
- Вызывается при запуске игры для выбранного аккаунта (`app/src-tauri/src/game.rs`). Upstream проверяет все аккаунты при старте и раз в 7 суток с разносом 200 мс — **не реализовано в лаунчере**.

### 7.2 Что лаунчер делает с токеном при запуске игры [код: Connector.cs:316-326; наш код: `play::auth_env`, `launch::build_command`]
- Токен уходит **только через env** (8.2), и только если аккаунт выбран, нет `--no-connect` и `auth.mode != Disabled`.
  Тогда же `ROBUST_AUTH_USERID`, `ROBUST_AUTH_PUBKEY`, `ROBUST_AUTH_SERVER`.
- `auth.mode == Required` без аккаунта (и без `--no-connect`) → `AuthRequired` до загрузки.
- Гость: `ROBUST_AUTH_*` не передаются, имя — `guest_name` (по умолчанию `FALLBACK_USERNAME` = `JoeGenero`). Унаследованные `ROBUST_AUTH_*` удаляются из env.
- URL auth-сервера **не берётся из `/info`**: иначе сервер направил бы `ROBUST_AUTH_SERVER` на себя и получил токен. Только из наших зеркал (7.4).
- Подтверждение личности делает **клиент (Robust)**; протокол клиент-сервер-auth в лаунчере не нужен.

### 7.3 Хранение аккаунтов [код: upstream DataManager.cs:295,455-464; наш код: `accounts::CredentialStore`, `secret.rs`]
- Upstream хранит токены открытым текстом в SQLite (`Login(UserId, UserName, Token, Expires)`); пароль не хранит.
- Мы: **Windows Credential Manager** (generic, `LOCAL_MACHINE`). Target `BebraLauncher/account/<user id>`, blob — JSON `StoredAccount {user_id, username, token, expire_time}`,
  ≤ 2560 байт. При чтении пропускается элемент с несовпадающим `user_id` или битый.
- Пароль уходит только в `authenticate`, не хранится и не логируется. `Token`: `Debug`/`Display` его не выводят, сравнение константное.

### 7.4 `ROBUST_AUTH_SERVER` [код: ConfigConstants.cs:12-16 `GetMostSuccessfulUrl`; наш код: `UrlFallbackSet::most_successful`]
Из двух auth-URL тот, у которого больше 2xx-ответов (при равенстве — основной). `ROBUST_AUTH_SERVERS` = `unset@<тот же URL>` (8.2). Смысл формата у клиента — [?].

### 7.5 Публичные ключи подписи [код: signing_key; наш код: `signature.rs`, `launch::ensure_signing_key_file_for_engine`]
- `signing_key` (PEM SPKI, Ed25519): `MCowBQYDK2VwAyEApQ9mAhMLbmhQqRH7itgNo75S5rCSMsMXvVRmMv1d9NQ=`. Совпадает с `crates/core/src/signing_key.pem` и с upstream
  (`raw.githubusercontent.com/space-wizards/SS14.Launcher/master/SS14.Launcher/signing_key`). Подпись реального движка проверена [live].
- Движки `multiverse` и `supermatter` — свои ключи (`signing_key_multiverse.pem`, `signing_key_supermatter.pem`). Источник не описан в доке [?].
- Файл записывается в `<root>\signing_key[_<engine>]` атомарно при каждом запуске, если содержимое другое; путь — аргумент 3 Loader.
- Проверяют: Loader — движок при каждом запуске (выключателя нет); core — движок после скачивания (6.2) и модули (6.3).

---

## 8. Запуск клиента [код: Connector.cs:271-398, 499-621; SS14.Loader/*; наш код: `launch.rs`, `play.rs`, `loader/`]

### 8.1 Loader — отдельная программа
`SS14.Loader.exe` — .NET 10 в `loader/` (часть файлов upstream, MIT; наши отличия — в `loader/README.md`). Делает (`loader/SS14.Loader/Program.cs`):
1. Читает ключ (аргумент 3), проверяет Ed25519-подпись **всего** engine-zip по `sig` (аргумент 2). Проверка и парсинг zip идут по одному буферу.
   Провал → код **2**. `SS14_DISABLE_SIGNING` удалён: отключить проверку нельзя.
2. Открывает zip, находит `Robust.Client.dll`, создаёт точку входа (`LoaderEntryPointAttribute`) [код: upstream, не проверено нами].
3. Применяет патчи клиента из `SS14_LOADER_PATCHES` (.dll через `;`, Harmony) — до старта движка.
4. Монтирует оверлей (`SS14_LOADER_OVERLAY_ZIP`, поверх игры), затем `content.db` (`SS14_LOADER_CONTENT_DB` + `SS14_LOADER_CONTENT_VERSION`).
   Пишет `RunningClient` (читает cull, 5.4). Вызывает `Main` движка.
5. Коды выхода: `0` ок; `1` меньше трёх аргументов; `2` подпись; `3` `Run()` неуспешен; `4` необработанное исключение.

**Redial** [код: upstream `RedialApi.cs`; наш код: `loader/SS14.Loader/RedialApi.cs`, `app/src-tauri/src/redial.rs`]:
клиент запускает **лаунчер**: `$SS14_LAUNCHER_PATH --commands :RedialWait R<hex причины> C<hex адреса>` (из env перед запуском удаляются `ROBUST_AUTH_*`, `SS14_LOADER_*`, `SS14_DISABLE_SIGNING`).
Команды: `:Ping`, `:RedialWait`, `R<hex>`/`r<текст>` (причина; текст от сервера недоверенный), `C<hex>`/`c<адрес>`.
У нас вместо named pipe — `tauri-plugin-single-instance` (argv второго процесса передаётся первому). Вживую не проверено; разбор покрыт тестами.
`ss14://…` одним аргументом только открывает сервер без подключения (upstream подключается сразу).

### 8.2 Запуск процесса [код: Connector.cs:499-621; наш код: `launch::build_command`, `launch::spawn`]
Путь к Loader: `BEBRA_LOADER` (имя обязано быть `SS14.Loader.exe`), затем `loader\` рядом с exe, затем `loader\out` в предках (dev).
`Bebra.HwidPatch.dll` и `Bebra.PerfPatch.dll` ищутся рядом с Loader.

```
SS14.Loader.exe <engine zip> <sig hex> <signing_key> --username <имя> --cvar display.compat=<True|False> --cvar launch.launcher=true
  [--launcher --connect-address <udp://…> --ss14-address <ss14[s]://…>]   # без них — главное меню
  [--cvar build.<name>=<value> …]    # download_url, manifest_url, manifest_download_url, version, fork_id, hash, manifest_hash, engine_version; только непустые
  [--cvar …]                         # профиль оптимизации
  [--cvar name=value …]              # пользовательские
```
- `--launcher` подразумевает `--connect`. `display.compat=True` — режим совместимости (GLES2).
- Профиль оптимизации — набор независимых переключателей `launch::OptimizationConfig`; какие `--cvar` даёт каждый, видно в `OptimizationConfig::cvars`
  (`light.*`, `parallax.low_quality`, `particles.*`, только косметические `accessibility.disable_*`, `render.tile_edges`, `audio.*`/`ambience.cooldown`,
  `display.vsync`/`display.max_fps`). Пользовательские `--cvar` идут после профиля и перекрывают его.
- Пользовательские аргументы: только `--cvar name=value` (имя `[A-Za-z0-9_.]`, ≤ 100; значение ≤ 500 без управляющих). Остальное отбрасывается.

Переменные окружения [код: Connector.cs:316-326, 520-563; наш код: `launch.rs`]:

| Переменная | Значение |
|---|---|
| `ROBUST_AUTH_TOKEN` | токен (секрет; в логах не печатается). Только если аккаунт передаётся (7.2) |
| `ROBUST_AUTH_USERID` | GUID аккаунта |
| `ROBUST_AUTH_PUBKEY` | `auth.public_key` из `/info` |
| `ROBUST_AUTH_SERVER` | самый успешный auth-URL (7.4) |
| `ROBUST_AUTH_SERVERS` | `unset@<тот же URL>` |
| `SS14_LOADER_CONTENT_DB` | путь к `content.db` (всегда) |
| `SS14_LOADER_CONTENT_VERSION` | `ContentVersion.Id` (всегда) |
| `SS14_LOADER_OVERLAY_ZIP` | опционально: zip текстовых пакетов (`core::packs`) |
| `SS14_LOADER_PATCHES` | опционально: пути к .dll патчей через `;` |
| `ROBUST_MODULE_<NAME>` | каталог модуля: `.`→`_`, верхний регистр, `_`→`__` (`A_B` → `A__B`). `Robust.Client.WebView` → `ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW` |
| `SS14_LAUNCHER_PATH` | путь к exe лаунчера (redial), опционально |
| `BEBRA_HWID_MODERN`, `BEBRA_HWID_LEGACY` | опционально (задан HWID); читает патч `Bebra.HwidPatch` |
| `DOTNET_TieredPGO=1`, `DOTNET_ReadyToRun=0`, `DOTNET_MULTILEVEL_LOOKUP=0` | всегда |
| `DOTNET_gcServer=1`, `DOTNET_GCNoAffinitize=1` | переключатель `server_gc` |
| `BEBRA_OPTIMIZE=1`, `DOTNET_GCRetainVM=1` | переключатель `low_latency_gc` |
| `BEBRA_PERF_OUTLINE=1`, `BEBRA_PERF_STATUS_ICONS=1` | переключатели `patch_outline`, `patch_status_icons`; читает патч `Bebra.PerfPatch` |
| `DOTNET_ROOT` | если задан `dotnet_root`; `play::prepare` сейчас его не задаёт |
| `DOTNET_ROLL_FORWARD` | **не ставить** (Loader всегда на runtime лаунчера); удаляется из env |

Остальные имена из `CLEARED_ENV` (включая `SS14_DISABLE_SIGNING`) удаляются из окружения процесса, если мы их не задали.

Запуск [код: Connector.cs; наш код: `launch::spawn`]:
- stdin — null; stdout/stderr — файлы. Приложение пишет каждую игру в свои `logs\client-<started_at>-<session>.stdout.log` / `.stderr.log`
  (общее имя убрано, P2-40). Файл дописывается; при ≥ 8 МиБ переименовывается в `.1`.
- Приоритет: `AboveNormal` (`0x8000`) при `high_priority`; `High` (`0x80`) только для профиля `optimized`.
- Лог команды (`describe()`): программа, аргументы, **имена** env. Значения не выводятся.
- Upstream ждёт 300 мс и отличает «упал сразу» по коду выхода — **не реализовано в лаунчере**. Завершение логируется (`game.rs`: `game pid … exited, code …`).

### 8.3 Нужный .NET
- Loader: TFM `net10.0` (`loader/Directory.Build.props`, `loader/SS14.Loader/SS14.Loader.csproj`). Dev-публикация:
  `dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained false -o loader/out`
  (нужен .NET 10 runtime, `loader/README.md`). Установщик self-contained (`scripts/build-release.ps1`) [?].
- `DOTNET_ROOT` лаунчер не выставляет (`dotnet_root: None`). Upstream-схема (`dotnet_x64\`, Bootstrap, `download_net_runtime.py`, `publish.py`, `VcRedistCheck.cs`, `Launcher.props`) в репозитории отсутствует — **не реализовано в лаунчере**.
- Нативное: SQLite (SQLitePCL.Batteries_V2), Harmony (`Lib.Harmony` 2.4.3, нужен для .NET 10). VC++ redistributable: [?].

---

## 9. Выводы для нашей архитектуры и принятое решение

**Core (Rust) реализован:** хаб, `/status`, `/info` (1); выбор режима, zip и manifest (2-4); ContentDB по схеме 5.2 (5);
движок и модули с SHA-256 и Ed25519 (6); аккаунты и токены (7); команда запуска и env (8).

**Loader на Rust не пишем:** он хостит управляемый Robust.Client.dll в .NET-процессе. Принят вариант A: `SS14.Loader` собирается из `loader/`
(upstream MIT с нашими отличиями, см. `loader/README.md`) через `dotnet publish` и поставляется с .NET 10 runtime.
Варианты B (скачивать Loader у оф. лаунчера) и C (переписать на .NET) отклонены.

Вес: в установщике Loader self-contained (~96 МБ, NOTES.md). [?] Оценка .NET runtime + Loader не замерялась.

**Тесты протокола:** BLAKE2b-256 и SHA-256; парсер манифеста и его хеш; поток блобов (`PreCompressed`, обрывы, лимиты); zip-режим;
`/info`, `/status`, хаб (включая 429); миграции и cull; Ed25519; разбор redial; `module_env_name`; `build_command`.
