# Project rules

- One process owns every audio stream and Twitch connection. The extension and
  the OBS overlay only read the local HTTP API; renderers never talk to Twitch.
- `crates/app` owns the single Router, config, log, tray and shutdown.
  `crates/music` and `crates/chat` export handlers and state, never bind.
- Bind the local server to `127.0.0.1` only. Port `45890`.
- State is guarded by `std::sync::nonpoison::{Mutex, RwLock}`. Poison handling
  does not belong in this codebase.
- Tokens live in `config.json` next to the binary. Never log them, never return
  them from the API, never commit the file.
- Static assets are built by `frontend/` into `crates/app/assets/app`. Do not
  add hand-written HTML pages next to the sources.
- Twitch redirect URI is `http://localhost:45890/chat/auth/callback`.
