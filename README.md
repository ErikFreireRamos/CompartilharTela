# CompartilhadorDeTela

by Erik Freire Ramos

App para Windows de transmissão de tela com áudio entre amigos: até 10 pessoas por sala, várias transmitindo ao mesmo tempo, sem microfone nem câmera.

- **Criar sala:** seu PC vira o host e gera um código, ex.: `P4JR-VJYK-BZWE`.
- **Entrar em sala:** cole o código.
- **Áudio do PC sem o Discord:** o áudio é capturado por aplicativo. Discord, Discord PTB/Canary e Vesktop vêm bloqueados, e dá para bloquear qualquer outro app.
- **Qualidade:** de 480p30 até 1440p60. O tipo "Jogo/vídeo" usa H.264 por hardware (Quick Sync/NVENC/AMF); "Texto" usa VP9.
- **Volume:** cada pessoa ajusta o volume de cada transmissão (0–200%), e quem transmite ajusta o volume enviado.
- **Troca de host automática:** se o host sair, quem estiver apto assume e todos reconectam sozinhos.

## Como funciona

```
[app de cada amigo] ──ws/http──► TCP 7880  proxy + API (no app do host)
                                  └─► livekit-server.exe (servidor de mídia, só repassa)
                    ══ mídia ═══► UDP 7882 (ou TCP 7881)
```

- **Servidor de mídia:** o [LiveKit](https://github.com/livekit/livekit) roda embutido no app de quem é host. Ele só repassa vídeo e não recodifica, então gasta pouca CPU. O que limita a sala é o **upload** do host (a tela de compartilhar mostra uma estimativa).
- **Código da sala:** leva o IP público do host e um segredo, sem nenhum serviço externo. Quando o host muda, o código muda junto, e a sala sempre mostra o código atual.
- **Áudio por app:** fica no `audio-capture.exe` (C++), que usa o *process loopback* do WASAPI (Windows 10 2004+ ou 11). Se não funcionar, o app transmite o áudio do PC inteiro e avisa na tela.

## Rede (importante para quem for host)

- **Portas:** TCP **7880–7881** e UDP **7882** precisam chegar ao PC do host.
  - O app tenta abrir essas portas sozinho via **UPnP**.
  - Se o roteador não aceitar, aparece um aviso na sala, e você abre as portas manualmente uma vez no roteador.
  - O instalador cria as regras do Firewall do Windows.
- **IP público:** sem IP público (CGNAT) não dá para ser host pela internet. Nesse caso a sala só funciona na rede local.
- **Quem pode ser host:** quando alguém entra, o host testa se as portas dessa pessoa estão acessíveis. Quem passa aparece como **"reserva"** na lista e pode assumir o host. Se ninguém passar no teste e o host sair, a sala acaba.
- **Mesma casa do host:** amigos na mesma rede local do host conectam pelo IP local automaticamente (descoberta por UDP 7883).

## Desenvolvimento

Requisitos: Node 22.12+, Visual Studio Build Tools 2022 com a carga **C++** (para o `audio-capture.exe`) e Windows 10/11.

```bash
cd app
npm install
npm run setup:livekit   # baixa o livekit-server.exe oficial e confere o SHA-256
npm run build:audio     # compila native/audio-capture -> resources/bin/audio-capture.exe
npm run dev             # abre o app em modo desenvolvimento
```

> **"Could not start video source" ao compartilhar:** o app está rodando como **administrador**, e nesse modo o Windows bloqueia a captura de tela. Isso acontece, por exemplo, se o VS Code estiver configurado para sempre abrir como administrador: tudo o que você roda no terminal dele herda isso. Rode o `npm run dev` num terminal normal ou tire o VS Code do modo administrador. O app mostra um aviso quando detecta esse caso.

### Outros comandos

| Comando | O que faz |
|---|---|
| `npm test` | testes unitários (código da sala, eleição do host) |
| `INTEGRATION=1 npx vitest run src/main/host` | sobe o LiveKit de verdade e testa API, proxy e tokens |
| `npm run typecheck` | checagem de tipos |
| `npm run dist` | gera o instalador em `app/dist/CompartilhadorDeTela-Setup-<versão>.exe` |

O instalador não é assinado. No primeiro uso o Windows mostra o aviso do SmartScreen: clique em "Mais informações" e depois em "Executar assim mesmo".

### Testar duas cópias no mesmo PC

```bash
npm run build
npx electron .                  # cópia A (crie a sala)
npx electron . --profile=b      # cópia B (entre com o código)
```

A cópia B usa outra pasta de configuração. Ela não consegue ser host enquanto a A estiver aberta, porque as portas são as mesmas. Por isso a **troca de host** só pode ser testada com dois PCs.

`CT_NO_UPNP=1` faz o app não mexer no roteador. É útil para testes automatizados.

## Estrutura

```
app/
  src/main/          processo principal: host (LiveKit, proxy, API, fila de sucessão),
                     rede (UPnP, STUN, descoberta local), áudio (controle do audio-capture.exe)
  src/preload/       ponte segura main <-> interface
  src/renderer/      interface React: telas, sessão LiveKit, troca de host, worklet de áudio
  src/shared/        código da sala, protocolo, tipos da API
  build/             script do instalador (firewall)
native/audio-capture/  captura WASAPI por processo + mixer (C++)
scripts/             download do LiveKit
```
