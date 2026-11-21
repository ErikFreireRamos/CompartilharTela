# CompartilhadorDeTela (Rust P2P Native)

Aplicativo nativo em Rust para transmissão de tela com áudio por aplicativo entre amigos via **Iroh P2P (QUIC)** — sem abrir portas no roteador, compatível com CGNAT e com troca automática de host.

## Arquitetura do Workspace

- `crates/ct-core` — Protocolo P2P, codecs de áudio (IMA-ADPCM 4:1, PCM 16-bit e resampler estéreo), qualidades independentes de vídeo/FPS/áudio e armazenamento atômico de configurações.
- `crates/ct-media` — Transporte Iroh P2P (`VideoMailbox` sem descarte de keyframes, streams ordenados de vídeo/áudio e handoff automático de host), captura nativa Windows DXGI Desktop Duplication (com seleção de GPU e fallback GDI), captura WASAPI por aplicativo com bloqueio ao vivo e mixer WASAPI com jitter buffer.
- `crates/ct-app` — Interface nativa `eframe`/`egui` (`glow`) com layout Destaque/Grade, tela cheia, janelas separadas (pop-out), aba lateral de participantes minimizável e controles ao vivo de resolução, FPS, áudio e apps bloqueados.

## Como compilar e testar

```bash
cd rust
cargo test --workspace
cargo build --release --bin compartilhador-de-tela
```
