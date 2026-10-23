#include "processes.h"

#include <audiopolicy.h>
#include <mmdeviceapi.h>
#include <tlhelp32.h>
#include <wrl/client.h>

#include <algorithm>

using Microsoft::WRL::ComPtr;

ProcMap SnapshotProcesses() {
  ProcMap procs;
  HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snapshot == INVALID_HANDLE_VALUE) return procs;
  PROCESSENTRY32W entry{};
  entry.dwSize = sizeof(entry);
  if (Process32FirstW(snapshot, &entry)) {
    do {
      procs[entry.th32ProcessID] = {entry.th32ParentProcessID, ToLower(entry.szExeFile)};
    } while (Process32NextW(snapshot, &entry));
  }
  CloseHandle(snapshot);
  return procs;
}

std::vector<DWORD> Ancestors(const ProcMap& procs, DWORD pid) {
  std::vector<DWORD> chain;
  DWORD current = pid;
  // limite de profundidade e checagem de ciclo: PIDs são reaproveitados pelo Windows
  for (int depth = 0; depth < 64; ++depth) {
    const auto it = procs.find(current);
    if (it == procs.end()) break;
    const DWORD parent = it->second.parent;
    if (parent == 0 || parent == pid || procs.find(parent) == procs.end()) break;
    if (std::find(chain.begin(), chain.end(), parent) != chain.end()) break;
    chain.push_back(parent);
    current = parent;
  }
  return chain;
}

std::vector<SessionInfo> EnumerateAudioSessions() {
  std::vector<SessionInfo> result;
  ComPtr<IMMDeviceEnumerator> enumerator;
  if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, IID_PPV_ARGS(&enumerator)))) {
    return result;
  }
  ComPtr<IMMDeviceCollection> devices;
  if (FAILED(enumerator->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &devices))) return result;
  UINT deviceCount = 0;
  devices->GetCount(&deviceCount);

  for (UINT i = 0; i < deviceCount; ++i) {
    ComPtr<IMMDevice> device;
    if (FAILED(devices->Item(i, &device))) continue;
    // um gerenciador novo a cada consulta: o enumerador de um gerenciador antigo
    // não enxerga sessões criadas depois
    ComPtr<IAudioSessionManager2> manager;
    if (FAILED(device->Activate(__uuidof(IAudioSessionManager2), CLSCTX_ALL, nullptr,
                                reinterpret_cast<void**>(manager.GetAddressOf())))) {
      continue;
    }
    ComPtr<IAudioSessionEnumerator> sessions;
    if (FAILED(manager->GetSessionEnumerator(&sessions))) continue;
    int sessionCount = 0;
    sessions->GetCount(&sessionCount);

    for (int j = 0; j < sessionCount; ++j) {
      ComPtr<IAudioSessionControl> control;
      ComPtr<IAudioSessionControl2> control2;
      if (FAILED(sessions->GetSession(j, &control)) || FAILED(control.As(&control2))) continue;
      if (control2->IsSystemSoundsSession() == S_OK) continue;
      AudioSessionState state;
      if (FAILED(control->GetState(&state)) || state == AudioSessionStateExpired) continue;
      DWORD pid = 0;
      if (FAILED(control2->GetProcessId(&pid)) || pid == 0) continue;

      const bool active = state == AudioSessionStateActive;
      const auto it = std::find_if(result.begin(), result.end(), [pid](const SessionInfo& s) { return s.pid == pid; });
      if (it == result.end()) {
        result.push_back({pid, active});
      } else {
        it->active = it->active || active;
      }
    }
  }
  return result;
}
