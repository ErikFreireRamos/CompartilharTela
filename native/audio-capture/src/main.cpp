// audio-capture.exe: captura o áudio do PC por app, sem os apps bloqueados.
//
// Comandos (stdin, um JSON por linha):
//   {"cmd":"list","blocked":["discord.exe"],"excludePid":123}
//   {"cmd":"start","blocked":["discord.exe"],"only":[],"excludePid":123}
//   {"cmd":"setBlocked","blocked":["discord.exe","spotify.exe"]}
//   {"cmd":"setOnly","only":["game.exe"]}
//   {"cmd":"pidOfWindow","hwnd":327750}
//   {"cmd":"stop"}
// `only` não vazio = captura só aqueles apps (e os filhos deles), ignorando `blocked`.
// Eventos (stdout, ver common.h): apps, started, stopped, error, log, pid; mais o PCM.
// Fecha sozinho quando o stdin fecha (o app saiu).
#include "common.h"
#include "capture.h"
#include "engine.h"
#include "processes.h"

namespace {

std::vector<std::wstring> ReadExes(const std::string& line, const char* key) {
  std::vector<std::wstring> exes;
  for (const std::string& exe : JsonGetStringArray(line, key)) exes.push_back(ToLower(FromUtf8(exe)));
  return exes;
}

/** Nome do executável (minúsculo) dono do processo. */
std::string ExeForPid(DWORD pid) {
  const ProcMap procs = SnapshotProcesses();
  const auto it = procs.find(pid);
  return it == procs.end() ? std::string() : ToUtf8(it->second.exe);
}

void HandleCommand(CaptureEngine& engine, const std::string& line) {
  const std::string cmd = JsonGetString(line, "cmd");
  const DWORD excludePid = static_cast<DWORD>(JsonGetInt(line, "excludePid", 0));
  if (cmd == "list") {
    SendJson(CaptureEngine::AppsJson(ReadExes(line, "blocked"), excludePid));
  } else if (cmd == "start") {
    if (!ProcessLoopbackSupported()) {
      SendJson(
          "{\"event\":\"error\",\"code\":\"unsupported\",\"message\":"
          "\"este Windows n\\u00e3o suporta captura de \\u00e1udio por app (precisa do Windows 10 2004+ ou 11)\"}");
      return;
    }
    engine.Start(ReadExes(line, "blocked"), ReadExes(line, "only"), excludePid);
    SendJson("{\"event\":\"started\"}");
  } else if (cmd == "setBlocked") {
    engine.SetBlocked(ReadExes(line, "blocked"));
  } else if (cmd == "setOnly") {
    engine.SetOnly(ReadExes(line, "only"));
  } else if (cmd == "pidOfWindow") {
    // o app manda o HWND que o Electron devolve no id da fonte (window:<hwnd>:0)
    const auto hwnd = reinterpret_cast<HWND>(static_cast<intptr_t>(JsonGetInt(line, "hwnd", 0)));
    DWORD pid = 0;
    if (hwnd) GetWindowThreadProcessId(hwnd, &pid);
    const std::string exe = pid ? ExeForPid(pid) : std::string();
    SendJson("{\"event\":\"pid\",\"pid\":" + std::to_string(pid) + ",\"exe\":" + JsonString(exe) + "}");
  } else if (cmd == "stop") {
    engine.Stop();
    SendJson("{\"event\":\"stopped\"}");
  }
}

}  // namespace

int wmain() {
  CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  CaptureEngine engine;
  HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
  std::string buffer;
  char chunk[4096];
  for (;;) {
    DWORD read = 0;
    if (!ReadFile(input, chunk, sizeof(chunk), &read, nullptr) || read == 0) break;
    buffer.append(chunk, read);
    for (size_t newline; (newline = buffer.find('\n')) != std::string::npos;) {
      HandleCommand(engine, buffer.substr(0, newline));
      buffer.erase(0, newline + 1);
    }
  }
  engine.Stop();
  CoUninitialize();
  return 0;
}
