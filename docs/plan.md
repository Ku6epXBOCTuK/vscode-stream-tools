# План: vscode-stream-tools

Объединение `music-player` и `twitch-chat` в один репозиторий, одно расширение и
один бэкенд-процесс.

## Цель

Один VS Code extension и один Rust-процесс, внутри которого живут музыкальный
плеер и Twitch-чат. Управление музыкой — из панели чата, без отдельной
HTML-страницы и без кнопок в статус-баре.

Четыре исходные причины:

1. Удобное управление вместо кнопок в статус-баре.
2. Плейлист и настройки через сворачивание/разворачивание, а не через отдельную
   страницу.
3. Команда `!трек` в чате, которая пишет текущий трек.
4. Общая база: расширение + Rust-бэкенд.

## Ключевые решения

| Решение      | Значение                                                      | Причина                                                    |
| ------------ | ------------------------------------------------------------- | ---------------------------------------------------------- |
| Репозиторий  | новый `vscode-stream-tools`                                   | старые два архивируются, история не переносится            |
| Бинарник     | один, `stream-tools.exe`                                      | по требованию; разделение — по крейтам                     |
| Порт         | один, `45890`                                                 | один процесс; redirect URI прописывается в Twitch один раз |
| Конфиг       | один `config.json` рядом с exe                                | миграции нет, при первом запуске дефолты                   |
| Лог          | один `stream-tools.log`, префиксы `music:` / `chat:`          | одна ротация, env `STREAM_TOOLS_LOG`                       |
| Трей         | один                                                          | Open panel / Play-Pause / Next track / Quit, тултип — трек |
| Ассеты       | `crates/app/assets/app`                                       | один статический каталог вместо двух                       |
| Идентичность | `.stream-tools` / `stream-tools-extension`, publisher прежний | новый `.vsix`, старые расширения удаляются                 |

История двух старых репозиториев не переносится: каталоги `extension/` и
`backend/` в них называются одинаково, склейка даёт либо мусорные пути, либо
коммиты-переезды. Новый репозиторий начинается одним начальным коммитом с
правильной структурой. Старые репозитории уходят в архив GitHub и остаются как
летопись.

## Структура

```text
vscode-stream-tools/
  crates/
    music/            либа: player, pump, audio, radio, streamer, типы плейлистов
    chat/             либа: auth, eventsub, store, sound, диспетчер команд
    app/              bin stream-tools: main, config, log, tray, Router, shutdown, health
  extension/          один .vsix "Stream Tools"
  frontend/           Svelte: панель + оверлей, outDir -> crates/app/assets/app
  justfile
  cog.toml
  AGENTS.md
  README.md
  pnpm-workspace.yaml
```

Крейты `music` и `chat` не поднимают собственный сервер. Они отдают
`pub`-хендлеры и своё состояние, а `app` собирает единственный Router и
монтирует пути. Поэтому конфиг, лог, трей, `/api/health` и `/api/shutdown`
существуют в одном экземпляре, а не по копии на крейт.

Имена:

- бинарник `stream-tools.exe`, установка `~/bin/stream-tools/`;
- `config.json` и `assets/` рядом с exe;
- лог `stream-tools.log`, env `STREAM_TOOLS_LOG`;
- переменная окружения для отладочного уровня — `STREAM_TOOLS_LOG`
  (debug/info/warn/error).

## Маршруты одного Router

```text
/api/health                     pid, exe, uptime + chat{...} + music{...}
/api/shutdown

/music/control
/music/volume
/music/now_playing
/music/playlists                GET, POST
/music/playlists/{index}        PATCH, DELETE
/music/stream                   в OBS: http://127.0.0.1:45890/music/stream

/chat/messages                  GET, POST
/chat/read
/chat/messages/{id}             DELETE
/chat/moderation/timeout
/chat/moderation/ban
/chat/channel
/chat/auth/url
/chat/auth/status
/chat/events                    SSE
/chat/eventsub/reconnect
/chat/badges
/chat/state

/chat/auth/callback              redirect URI Twitch

/panel
/overlay
/assets/*
/                        redirect -> /panel
```

Каждая группа живёт под своим путём: `/music/*` — музыкальный крейт, `/chat/*` —
чат. Вне групп остаются только общие для процесса `/api/health` и
`/api/shutdown`: вытаскивать `/chat/auth/callback` в корень ради одной строки в
консоли Twitch смысла нет.

`backend/assets/index.html` удаляется: страница настроек музыки переезжает в
панель.

## Конфиг

```json
{
  "version": 1,
  "port": 45890,
  "music": {
    "volume": 0.8,
    "shuffle": false,
    "current_playlist_index": 0,
    "playlists": []
  },
  "chat": {
    "channel": "",
    "client_id": "",
    "client_secret": "",
    "tokens": {},
    "widget": {},
    "sound": {}
  },
  "commands": {
    "track": {
      "enabled": true,
      "aliases": ["!трек", "!track"],
      "template": "Сейчас играет: {track}",
      "cooldown_seconds": 10
    }
  }
}
```

Сохранение атомарное, ротация `.bak` / `.bak.2` как в обоих старых бэкендах.
Порт `45890` зафиксирован константой по умолчанию, поле остаётся для ручной
правки.

Миграции нет. Первый запуск создаёт файл с дефолтами, дальше — «Sign in» в
Twitch и добавление плейлистов руками.

## Интерфейс

Порядок в панели чата: `StatusBar` → `MusicStrip` → лента → `Composer`. Ширина
сайдбара около 350 px.

Свёрнутое состояние, одна строка:

```text
[Playing] Artist - Track...        [>] [>>]  [v]
```

Развёрнутое, под ней:

```text
Playlist 1 (local)  -  12/48
Volume 80%  |  Shuffle off
+ добавить плейлист
```

Развёрнутая часть даёт выбор плейлиста, слайдер громкости, shuffle,
добавление/переименование/удаление плейлиста. Это новый компонент
`panel/components/Music.svelte`; отдельного `music.html` больше нет.

Статус-бар расширения: один элемент с иконкой чата и текущим треком, клик
фокусирует панель. Остальные семь кнопок музыки из статус-бара убираются, их
функции доступны в панели и в палитре команд.

## Команда !трек

Вызов внутри процесса: `app` передаёт в `chat` реализацию `NowPlayingSource`,
без HTTP-запросов.

Правила:

- срабатывает только в настроенном канале;
- свои сообщения пропускаются, иначе получается цикл;
- кулдаун из конфига;
- шаблон ответа из конфига, `{track}` подставляется;
- алиасы из конфига; дефолтный ASCII-алиас `!track` в коде, кириллический
  `!трек` живёт в конфиге;
- в лог пишется факт срабатывания и причина отказа.

## Этапы

```text
Этап 1  Создать репо, влить оба дерева, скелет crates/{music,chat,app},
        один Router/порт/конфиг/лог/трей, /music/*, health          3-4 дня
Этап 2  Один .vsix: лончер на один процесс, порт, статус-бар                1 день
Этап 3  MusicStrip под StatusBar, сворачиваемые секции, удалить index.html  1-2 дня
Этап 4  !трек ин-процессом, кулдаун, шаблон, тесты                          3-4 часа
Этап 5  Рецепты just, README, трекер, OBS-URL                               2-3 часа
```

Итого примерно 6-8 дней.

Детальный порядок с картой файлов и проверками — `migration.md`.

## Ручные шаги после этапа 2

1. Создать пустой репозиторий `vscode-stream-tools` на GitHub.
2. Архивировать `vscode-music-player` и `vscode-sidebar-twitch-chat`.
3. Удалить оба старых расширения из VS Code.
4. В консоли Twitch заменить redirect URI на
   `http://localhost:45890/chat/auth/callback`.
5. Поправить в OBS URL медиа-источника на `http://127.0.0.1:45890/music/stream`.

Redirect URI меняется один раз. Браузерный оверлей переезжает на
`:45890/overlay` сам.

## Известные ограничения

- Перезапуск бэкенда общий: чат и `/music/stream` умирают вместе.
- Сборка на ubuntu-раннере требует `libasound2-dev` из-за `rodio` в общем бинаре
  (VSC-45). На Windows это не проявляется.
- Паника в `tokio`-задаче процесс не убивает; реальные убийцы — `process::exit`,
  OOM и segfault в FFI (`tray-icon`, `cpal`, `rodio`).
- Состояние защищают непоизонящие локи `std::sync::nonpoison::{Mutex, RwLock}`
  (feature `sync_nonpoison`, новый Rust) вместо `std::sync::Mutex`/`RwLock`.
  Отравленных локов нет: `.expect("player state poisoned")` в музыкальном
  бэкенде и `PoisonError::into_inner` в чате при объединении убираются, лок
  всегда отдаётся напрямую. Паника под локом оставляет состояние как есть —
  согласованность восстанавливает следующий хендлер, а не разыменование
  `PoisonError`.

## Открытые вопросы

1. Что делать с проектами в трекере: `music-player` (MP) и `vsc-chat` (VSC) —
   архивировать, объединить в один или открытые задачи перенести.
2. Точный ID и publisher расширения при публикации.
3. Начальная версия нового репозитория: `0.1.0`, дальше только cog.
