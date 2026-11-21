# CompartilhadorDeTela (Rust Native)

Pure Rust native rewrite of CompartilhadorDeTela targeting Windows 10 (2004+) / Windows 11 with `eframe`/`egui` + `wgpu`, native Win32/WASAPI/DXGI APIs, and the embedded `livekit-server.exe` sidecar.

## Workspace Layout

- `crates/ct-core` — Protocol constants, Crockford Base32 room codes (IPv4/IPv6), deterministic host failover election, and atomic JSON settings store.
- `crates/ct-net` — Async STUN (RFC 5389), UPnP IGD (SSDP + `quick-xml` + SOAP), LAN UDP discovery (`7883`), native Win32 `GetAdaptersAddresses` (stable IPv6) and `GetTokenInformation` (elevation check).
- `crates/ct-host` — `axum` public server on `TCP 7880` (`/probe`, `/api/join`, HTTP & WebSocket proxy), UDP `7882` probe responder, `livekit-server` sidecar manager (with Win32 Job Object cleanup), LiveKit JWT minting, and Twirp RoomService client.
- `crates/ct-media` — In-process WASAPI audio capture with lock-free `ringbuf` (`48 kHz` stereo), monitor enumeration & screen capture, and `RoomSession` async state machine.
- `crates/ct-app` — Native `eframe`/`egui` + `wgpu` desktop application (`compartilhador-de-tela`).

## Build & Test

```bash
cd rust
cargo test --workspace
cargo build --release
```

### Enabling the LiveKit WebRTC Client (`livekit-rtc`)

On Windows (with MSVC Build Tools installed), enable the `livekit-rtc` feature in `ct-media` to link `libwebrtc`:

```bash
cargo build --release --features ct-media/livekit-rtc
```

Place `livekit-server.exe` in `bin/` next to the compiled `compartilhador-de-tela.exe` (or run `npm run setup:livekit` in `../app` and copy `app/resources/bin/livekit-server.exe`).
