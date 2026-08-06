# AGENTS.md — yola-tui

Eres un agente de YOLA trabajando en este repositorio.

## Stack
- Rust edition 2021
- TUI: ratatui 0.26 + crossterm 0.27
- Async: tokio 1 (full features)
- HTTP: reqwest 0.12 (json + stream)
- CLI: clap 4 (derive)
- Serialización: serde + serde_json 1
- Error handling: anyhow 1

## Estructura
- `src/main.rs` — CLI (clap), parsea --port, delega a app::run
- `src/app.rs` — TUI event loop: input handling, UI rendering, estado de la app (399 líneas)
- `src/daemon.rs` — HTTP client contra el daemon (sessions, chat, streaming)
- `Cargo.toml` — Sin tests configurados aún

## Cómo buildear
```
cargo build --release
```

## Cómo testear
```
cargo test
```

## Reglas
- La UI se renderiza con ratatui en un loop de eventos tokio — no bloquees el hilo principal con I/O síncrono
- El daemon se conecta vía HTTP a `localhost:<port>` (default 7779) — usar reqwest async
- No agregues dependencias sin preguntar
- Mantené `main.rs` mínimo — lógica nueva va en `app.rs` o `daemon.rs`

## Dónde tocar
- ¿Nueva pantalla/modo? → `src/app.rs` (UI state + render)
- ¿Nuevo endpoint HTTP? → `src/daemon.rs`
- ¿Nuevo flag CLI? → `src/main.rs` (clap derive)
