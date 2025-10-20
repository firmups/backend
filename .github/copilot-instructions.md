## Copilot instructions for firmups-backend

This repository is a small Rust HTTP backend (single binary) implemented with axum and tokio.
Provide concise, codebase-specific edits only. Key files:

- `Cargo.toml` — dependencies and crate name (`firmups-backend`).
- `src/main.rs` — the entire HTTP entrypoint, routes, handlers, and simple data types.

Quick summary (big picture):

- Single-process HTTP server using `tokio` runtime and `axum` router.
- Main request flow: hyper listener -> axum `Router` -> handler functions -> use `axum::Json` extractors/serializers.
- Logging uses the `log` facade with `env_logger::init()` in `main`.
- The app binds to `127.0.0.1:3000` (see `src/main.rs`) — be careful when changing bind address.

What to do when modifying code:

- Add routes to `src/main.rs` by extending the `Router::new()` chain. Follow the pattern:
  - `.route("/path", get(handler))` or `post(handler)`.
  - Handlers take typed extractors (e.g. `Json<CreateUser>`) and return types impl `IntoResponse`.
- Serialization: use `serde::{Deserialize, Serialize}` for request/response models (see `CreateUser` / `User`).
- Prefer returning `(StatusCode, Json<T>)` for clear status codes.

Build / run / debug notes (project-specific):

- Build: `cargo build` (or `cargo build --release` for production).
- Run locally with logging enabled: `RUST_LOG=info cargo run` — `env_logger` reads `RUST_LOG`.
- Backtraces: use `RUST_BACKTRACE=1` when debugging panics.
- Lint/format: `cargo clippy` and `cargo fmt` (not configured here but standard Rust tools apply).

Integration & dependencies:

- Key crates in `Cargo.toml`: `axum`, `tokio`, `serde`, `log`, `env_logger`.
- No database or external services in the current code; adding integrations should follow the async/tokio model.

Conventions and patterns discovered in this repo:

- Small, single-file prototype: most app logic lives in `src/main.rs`. If expanding, split handlers into `src/handlers.rs` and models into `src/models.rs`.
- Use axum extractors for parsing JSON input. Example: `async fn create_user(Json(payload): Json<CreateUser>) -> impl IntoResponse { ... }`.
- Return explicit status codes for clarity (e.g., `StatusCode::CREATED`).

Testing guidance (discoverable patterns only):

- There are no tests in the repository. For handlers, use `axum::response::Response` test helpers or spin up the `Router` in async tests with `tokio::test`.

When editing or adding files, reference these examples:

- `src/main.rs` lines that define routes: root route (`/`) and `POST /users` handler with `CreateUser`/`User` types.

What NOT to change without explicit reason:

- Do not change the runtime model (switching away from `tokio`/`async` or `axum`) without a clear migration plan — code assumes async everywhere.
- Avoid binding to 0.0.0.0 by default; tests and local dev expect `127.0.0.1:3000` unless intentionally opening network access.

If you need more context or want the agent to make broader structural changes (split modules, add DI, DB), ask for permission and specify the desired module layout and preferred crates.

Feedback: if any section seems off or you want the agent to follow stricter rules (naming, module layout), tell me which convention to enforce and I'll update these instructions.
