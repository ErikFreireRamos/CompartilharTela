// audio-capture.exe: captura o áudio do PC por app, sem os apps bloqueados.
//
// Comandos (stdin, um JSON por linha):
//   {"cmd":"list","blocked":["discord.exe"],"excludePid":123}
//   {"cmd":"start","blocked":["discord.exe"],"excludePid":123}
//   {"cmd":"setBlocked","blocked":["discord.exe","spotify.exe"]}
//   {"cmd":"stop"}
// Eventos (stdout, ver common.h): apps, started, stopped, error, log; mais o PCM.
// Fecha sozinho quando o stdin fecha (o app saiu).
#include "common.h"
#include "capture.h"
#include "engine.h"

namespace {

std::vector<std::wstring> ReadBlocked(const std::string& line) {
  std::vector<std::wstring> blocked;
  for (const std::string& exe : JsonGetStringArray(line, "blocked")) blocked.push_back(ToLower(FromUtf8(exe)));
  return blocked;
}

void HandleCommand(CaptureEngine& engine, const std::string& line) {
  const std::string cmd = JsonGetString(line, "cmd");
  const DWORD excludePid = static_cast<DWORD>(JsonGetInt(line, "excludePid", 0));
  if (cmd == "list") {
    SendJson(CaptureEngine::AppsJson(ReadBlocked(line), excludePid));
  } else if (cmd == "start") {
    if (!ProcessLoopbackSupported()) {
      SendJson(
          "{\"event\":\"error\",\"code\":\"unsupported\",\"message\":"
          "\"este Windows n\\u00e3o suporta captura de \\u00e1udio por app (precisa do Windows 10 2004+ ou 11)\"}");
      return;
    }
    engine.Start(ReadBlocked(line), excludePid);
    SendJson("{\"event\":\"started\"}");
  } else if (cmd == "setBlocked") {
    engine.SetBlocked(ReadBlocked(line));
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
