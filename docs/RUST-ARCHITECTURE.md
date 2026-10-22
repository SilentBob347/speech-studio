# Границы Rust после B01–B10

Сервис остаётся одним crate `speech-server`. Модули объявляются через
`имя.rs` + `имя/`. Файл подсистемы задаёт её интерфейс: закрытые `mod`
содержат реализацию, `pub(crate) use` открывает выбранные операции и типы,
а небольшие пространства имён объединяют связанные точки входа.

## Сборка приложения

| Файл | Ответственность |
|---|---|
| [lib.rs](../crates/speech-server/src/lib.rs) | Объявления подсистем и совместимые публичные экспорты для бинарника и desktop |
| [app/state.rs](../crates/speech-server/src/app/state.rs) | `AppState` и создание общих зависимостей |
| [app/bootstrap.rs](../crates/speech-server/src/app/bootstrap.rs) | Tokio, слушатель, Hub, возобновление фонового наблюдения, запуск сервера |
| [app/router.rs](../crates/speech-server/src/app/router.rs) | REST/SSE/MCP, middleware, лимиты тела и подключение исполнителя инструментов |
| [app/runtime.rs](../crates/speech-server/src/app/runtime.rs) | Пути инструментов, настройка ORT, библиотек и прокси при запуске |
| `app/*_api.rs`, [app/files.rs](../crates/speech-server/src/app/files.rs) | Входные HTTP-адаптеры, выбор папки и отдача файлов |

`AppState` используется при сборке и в HTTP-адаптерах. Фоновая работа получает
нужные пути, очередь и параметры: например, `speech::synthesis` получает
`&SpeechPaths` и `&JobQueue`, защита микрофона — путь workspace, а наблюдение
Listening — `watching::Dependencies`.

## Интерфейсы подсистем

| Подсистема | Открыто соседям | Закрытая реализация |
|---|---|---|
| [speech.rs](../crates/speech-server/src/speech.rs) | Хранение и подготовка, создание из транскрипта, `SpeechPaths`, правила режиссёра; `http`, `talk`, `stream` | `project`, `storage`, `api`, `jobs`, `text`, `say`, `live`, `director` и остальные файлы Speech |
| [listening.rs](../crates/speech-server/src/listening.rs) | Хранение и подготовка источника, `ListenProject`, `ListenPaths`, сводка, экспорт транскрипта, остановка записи; `http`, `watching`, `recognition`, `subtitles` | `project`, `storage`, `api`, `jobs`, `watch`, `live`, `recording`, экспорт и подготовка источника |
| [voices.rs](../crates/speech-server/src/voices.rs) | Выбранные операции библиотеки и референсов, `VoicePaths`; `http`, `matching`, возможности `record`, `fx`, `ref_pick` | `library`, `reference`, `profiles`, `jobs`, `api` |
| [app.rs](../crates/speech-server/src/app.rs) | Контекст, события, ревизии, подготовка компонентов и межфункциональные сценарии; `recognition`, `profiles`, `remote`, `costs`, `google`, `llm`, `voice_synthesis`, `agent` | HTTP-адаптеры, конкретные сценарии, Hub, журнал, настройки, загрузки, сборка приложения |

Пространства `http`, `talk`, `stream` и остальные перечисленные интерфейсы
содержат явные реэкспорты выбранных символов. Они не открывают целиком файлы
реализации. Экспорты, нужные только соседним тестам, ограничены `#[cfg(test)]`.

У технических подсистем `audio`, `text`, `engines`, `integrations`, `platform`,
а также у `jobs` и `setup`, сохраняются доступные внутри crate модули возможностей.
Например, `platform::fs`, `text::prep`, `engines::stress`, `integrations::cloud`.
Их вызывают по пути владельца; отдельный корневой алиас для каждого файла не нужен.
Это не тотальное сокрытие каждого Rust-модуля.

## Направления зависимостей

| Потребитель | Разрешённый способ обращения |
|---|---|
| Бинарник и desktop | Публичный интерфейс `speech_server`: `AppState`, `JobQueue`, `build_router`, `serve`, `serve_blocking`, `service`, `diagnostics`, `i18n`, `net`, `process_group`, `apply_faces_backend`, `flush_hub_on_exit` и функции настройки окружения |
| Сборка приложения и роутер | Интерфейсы подсистем и собственные закрытые адаптеры `app` |
| Сценарий `app`, работающий со Speech/Listening/Voices | Выбранные операции и типы фасада, без `speech::project`, `listening::jobs` или `voices::library` |
| Подсистема, вызывающая сервис приложения | Экспорт из `app`, например `app::recognition::transcribe` или `app::save_with_revision`, без доступа к `app::asr` и `app::revisions` |
| Фоновая задача | Операция владельца и конкретные зависимости; остановка записи вызывает `listening::stop_recording`, а не HTTP-обработчик |
| Общие преобразования и модели данных | Чистые входные данные и описания; файловые операции и выбор возможностей остаются у владельцев сценариев |
| Модуль внутри своей подсистемы | Собственные закрытые дочерние модули и интерфейсы соседних подсистем |

Это границы доступности внутри одного crate, а не обещание ацикличного графа
всех возможностей. Существующие HTTP-сценарии Live используют выбранные обработчики
`listening::http`; их дальнейшее разделение относится к работе над жизненным циклом
микрофона. B10 убирает обращение фоновой защиты микрофона к HTTP.

Например, соседний модуль сохраняет проект через `crate::speech::save(...)`,
а не через `crate::speech::storage::save(...)`. Для записи общего файла он вызывает
`crate::platform::fs::write_atomic(...)`. Если нужна новая операция, её явно добавляют
в фасад владельца; открывать весь внутренний модуль ради одного потребителя не следует.

## Проверка изменений

Компилятор проверяет закрытость дочерних модулей. Инвентаризация в
[app/tools/tests.rs](../crates/speech-server/src/app/tools/tests.rs) читает
`app/router.rs` и сверяет маршруты с инструментами в обе стороны. Проверки полного
роутера в [app/router/tests.rs](../crates/speech-server/src/app/router/tests.rs)
покрывают Host/Origin, CORS, методы, авторизацию OpenAI, MCP и лимиты тела.
Проверяются также SSE, Range-ответы и контракты ошибок перенесённых операций.
Обычная компиляция desktop проверяет сохранённые публичные экспорты.

План, результаты и прежние функциональные ограничения хранения, кэша и расходов —
в [RUST-BOUNDARY-PLAN.md](RUST-BOUNDARY-PLAN.md). Завершение структурных границ
не добавляет ProjectStore, CAS или новые гарантии конкурентного сохранения.
