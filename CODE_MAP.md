# CODE_MAP — yola-tui

> Auto-generated from filesystem. Todo afirmación es `[V]` (verificada en disco).

---

## 1. Stack

| Capa | Tecnología | Versión | Evidencia |
|------|-----------|---------|-----------|
| Lang | Rust | edition 2021 | `Cargo.toml` |
| Async runtime | Tokio | `1` (full features) | `Cargo.toml` |
| TUI framework | ratatui | `0.26` | `Cargo.toml` |
| Terminal backend | crossterm | `0.27` | `Cargo.toml` |
| HTTP client | reqwest | `0.12` (json + stream) | `Cargo.toml` |
| CLI parsing | clap | `4` (derive) | `Cargo.toml` |
| Serialization | serde + serde_json | `1` | `Cargo.toml` |
| Error handling | anyhow | `1` | `Cargo.toml` |
| Streams | futures | `0.3` | `Cargo.toml` |

---

## 2. Entry Point

**Archivo**: `src/main.rs` (30 líneas)

```
main.rs
├── mod app      ← app.rs
├── mod daemon   ← daemon.rs
│
├── struct Cli (clap::Parser)
│   ├── command: Option<Commands>  (subcomando opcional)
│   └── port: u16  (--port/-p, default 7779)
│
├── enum Commands (clap::Subcommand)
│   └── Chat  ← único subcomando, "Start interactive chat mode (default)"
│
└── #[tokio::main] async fn main()
    └── match cli.command.unwrap_or(Commands::Chat)
        └── Commands::Chat → app::run(cli.port).await
```

**CLI**: `yola-tui [--port 7779] [chat]` — `chat` es el subcomando por defecto.

---

## 3. Módulos (exhaustivo)

### 3.1 `src/main.rs` — CLI y arranque (30 líneas)

Configuración de clap con derive macros. Define `Cli` struct con flag `--port` y enum `Commands` con única variante `Chat`. `main()` hace parse y delega a `app::run()`.

### 3.2 `src/app.rs` — TUI app, event loop, UI (399 líneas)

#### Estado (structs)

```rust
struct App {
    input: String,              // buffer del input actual
    messages: Vec<Message>,     // historial de mensajes
    cursor_pos: usize,          // posición del cursor en input
    connected: bool,            // ¿daemon alcanzable?
    port: u16,                  // puerto del daemon
    session_id: Option<String>, // ID de sesión (creada lazy)
    status: String,             // texto de la barra de estado
    is_streaming: bool,         // ¿hay stream activo?
}

struct Message {
    role: String,   // "user" | "assistant" | "system"
    content: String,
}

enum AppEvent {
    Token(String),           // token recibido del stream
    Done,                    // stream terminado
    SessionCreated(String),  // sesión creada, payload = session_id
    Error(String),           // error ocurrido
    Quit,                    // salir
}
```

#### Entry point: `pub async fn run(port: u16) -> anyhow::Result<()>`

Flujo completo:
1. **Terminal setup**: `enable_raw_mode()` → `EnterAlternateScreen` + `EnableMouseCapture` → `CrosstermBackend` → `Terminal::new()`
2. **Health check**: `daemon::check_health(port)` → determina `connected`
3. **Init App**: mensaje "system" inicial con status, `session_id: None`
4. **Channel**: `mpsc::unbounded_channel::<AppEvent>()` para comunicación async→UI
5. **Event loop**: `event_loop(&mut terminal, &mut app, tx, &mut rx)`
6. **Restore**: `disable_raw_mode()` → `LeaveAlternateScreen` → `DisableMouseCapture` → `show_cursor()`

#### Event loop: `async fn event_loop()`

Loop principal (líneas 105-198):
- **Draw**: `terminal.draw(|f| ui(f, app))` en cada iteración
- **Keyboard poll** (50ms timeout):
  - `Esc` → `return Ok(())`
  - `Ctrl+C` → `return Ok(())`
  - `Enter` → `handle_enter(app, &tx)`
  - `Char(c)` → inserta en `app.input` en cursor_pos
  - `Backspace` / `Delete` → elimina carácter
  - `Left` / `Right` / `Home` / `End` → navegación cursor
- **Async events** (`rx.try_recv()`):
  - `SessionCreated(id)` → `app.session_id = Some(id)`
  - `Token(token)` → append al último mensaje si role == "assistant"
  - `Done` → `app.is_streaming = false`, status "Ready"
  - `Error(e)` → append al assistant bubble + mensaje "system" separado
  - `Quit` → `return Ok(())`

#### Enter handler: `fn handle_enter()`

(Líneas 202-229)
- Ignora si `is_streaming` o input vacío
- Toma el prompt con `std::mem::take` (resetea input)
- Añade `Message { role: "user" }` y `Message { role: "assistant", content: "" }`
- Setea `is_streaming = true`
- Spawnea tarea tokio: `stream_prompt(port, session_id, prompt, tx_clone)`

#### SSE streaming: `async fn stream_prompt()`

(Líneas 233-291)
1. **Lazy session**: si `session_id` es None → `daemon::create_session(port)` → envía `SessionCreated`
2. **POST prompt**: `daemon::send_prompt(port, &sid, &prompt)` → obtiene `reqwest::Response`
3. **Drain byte stream**: `response.bytes_stream()` → acumula en buffer String, splittea por `\n`
4. **Parse SSE**: cada línea no vacía → si empieza con `"data: "` → `process_sse_data(data, &tx)`
5. **On complete**: envía `AppEvent::Done`

#### SSE parser: `fn process_sse_data()`

(Líneas 294-332)
Intenta parsear `data` como JSON y busca en orden:

| Orden | JSON path | Emite |
|-------|-----------|-------|
| 1 | `choices[0].delta.content` (OpenAI shape) | `Token` |
| 2 | `token` (simple) | `Token` |
| 3 | `content` | `Token` |
| 4 | `text` | `Token` |
| 5 | `error` | `Error` |
| — | `[DONE]` | Nada (Done se emite al final del loop) |
| — | Non-JSON fallback | `Token` con el raw string |

**Formas JSON soportadas**: OpenAI/chat-completion, token simple, content, text, error. Fallback a texto plano.

#### UI: `fn ui(f: &mut Frame, app: &App)`

(Líneas 336-399)

**Layout** (vertical, 3 paneles):

| Panel | Constraint | Contenido |
|-------|-----------|-----------|
| Messages | `Min(1)` | `Paragraph` con título "YOLA Chat :{port}". Cada mensaje coloreado: user=Green, assistant=White, system=DarkGray. User messages prefijadas con "> ". Wrap enabled. |
| Input | `Length(3)` | `Paragraph` con título "Prompt". Amarillo (normal) o DarkGray (streaming). |
| Status bar | `Length(1)` | Texto DarkGray con `app.status`. |

**Cursor**: posicionado en `[input.x + cursor_pos + 1, input.y + 1]`.

### 3.3 `src/daemon.rs` — HTTP client (73 líneas)

**Constante**: `BASE_URL = "http://localhost"`

**Struct interna**: `SessionResponse { session_id: String }` (deserialización)

#### Funciones públicas

| Función | Firma | Endpoint | Descripción |
|---------|-------|----------|-------------|
| `check_health` | `(port: u16) -> bool` | `GET /api/v1/health` | `true` si status success |
| `create_session` | `(port: u16) -> Result<String>` | `POST /api/v1/sessions` | Body vacío, headers Content-Type JSON. Retorna `session_id`. |
| `send_prompt` | `(port: u16, session_id: &str, prompt: &str) -> Result<Response>` | `POST /api/v1/sessions/{id}/chat` | Body `{"prompt":"..."}`, Accept `text/event-stream`. Retorna la Response (caller lee el stream). |

**Manejo de errores**: `create_session` y `send_prompt` validan status code y devuelven `anyhow::bail!` con status + body en caso de error.

---

## 4. Flujo de ejecución completo

```
[Inicio]
  │
  ├─ main.rs: Cli::parse()
  ├─ app::run(port)
  │
  ├─ enable_raw_mode() + EnterAlternateScreen
  ├─ daemon::check_health(port) → connected: bool
  ├─ Init App { messages: [system status], session_id: None }
  ├─ mpsc::unbounded_channel()
  │
  └─ event_loop()
       │
       ├─ terminal.draw(ui)  ← cada iteración
       ├─ poll keyboard (50ms)
       │   ├─ Esc/Ctrl+C → Quit
       │   ├─ Enter → handle_enter()
       │   │   └─ tokio::spawn(stream_prompt)
       │   │       ├─ [si no hay session] daemon::create_session()
       │   │       ├─ daemon::send_prompt() → SSE stream
       │   │       ├─ response.bytes_stream() → parse "data: " lines
       │   │       │   └─ process_sse_data() → Token/Error events
       │   │       └─ AppEvent::Done
       │   └─ chars/backspace/etc → input buffer
       │
       └─ rx.try_recv() → aplica AppEvents al estado
           ├─ SessionCreated → guarda id
           ├─ Token → append a último mensaje
           ├─ Done → is_streaming = false
           └─ Error → append + mensaje system

[Fin] → disable_raw_mode() + LeaveAlternateScreen
```

---

## 5. Endpoints del daemon consumidos

| Método | Ruta | Usado por | Propósito |
|--------|------|-----------|-----------|
| GET | `/api/v1/health` | `check_health()` | Health check inicial |
| POST | `/api/v1/sessions` | `create_session()` | Crear sesión (body vacío) |
| POST | `/api/v1/sessions/{id}/chat` | `send_prompt()` | Enviar prompt, recibir SSE |

---

## 6. Archivos de configuración

| Archivo | Contenido clave |
|---------|-----------------|
| `Cargo.toml` | 7 dependencias: ratatui, crossterm, tokio (full), reqwest (json+stream), serde/serde_json, clap (derive), anyhow, futures |
| `src/main.rs` | 30 líneas, clap derive, tokio::main |
| `src/app.rs` | 399 líneas, TUI completa |
| `src/daemon.rs` | 73 líneas, HTTP client |
