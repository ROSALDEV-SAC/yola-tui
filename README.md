# yola-tui -- Terminal UI for YOLA

Frontend de terminal para desarrolladores. Velocidad pura con teclado.
Chat interactivo con streaming SSE. No replica el OS web -- solo expone la
sesion de chat.

## Stack

- Rust
- ratatui (TUI framework)
- crossterm (terminal backend)
- reqwest (HTTP/SSE client)
- clap (argument parsing)

## Funcionalidad

Al arrancar (`yola-tui` o `yola-tui chat`):

1. **Health check** automatico al daemon en `localhost:7779` (`/api/v1/health`).
   Muestra estado de conexion en la barra de status.
2. **Panel superior**: historial de mensajes (user/assistant/system), scrollable.
3. **Panel inferior**: input buffer con cursor, soporte para edicion basica
   (Backspace, Delete, Left/Right, Home, End, Ctrl+U limpiar).
4. **Streaming SSE**: al enviar un prompt, se crea una sesion via
   `POST /api/v1/sessions`, luego se streamea el chat via
   `POST /api/v1/sessions/:id/chat`. Los tokens se renderizan en tiempo real.
5. **Atajos**: Enter para enviar, Esc para salir, Ctrl+C para salir.

## Compilacion

```bash
cargo build --release
cargo run
```

El binario se conecta al daemon via HTTP puro (sin dependencias internas de
YOLA -- `@yola/client` no es necesario porque el cliente esta en Rust).

## Dependencias externas

Ninguna. El binario es autocontenido (static linking via Rust). Solo necesita
network para hablar con el daemon YOLA.
