#include "engine.h"

#include <mmsystem.h>

#include <algorithm>
#include <map>
#include <unordered_set>

#include "processes.h"

namespace {

constexpr DWORD kRefreshMs = 1000;
constexpr ULONGLONG kRetryFailedMs = 10000;

bool IsBlockedExe(const std::wstring& exe, const std::vector<std::wstring>& blocked) {
  return std::find(blocked.begin(), blocked.end(), exe) != blocked.end();
}

bool IsBlockedRoot(const ProcMap& procs, DWORD pid, const std::vector<std::wstring>& blocked, DWORD excludePid) {
  if (pid == excludePid) return true;
  const auto it = procs.find(pid);
  return it != procs.end() && IsBlockedExe(it->second.exe, blocked);
}

/** O processo, ou algum ancestral dele, está bloqueado. */
bool InBlockedTree(const ProcMap& procs, DWORD pid, const std::vector<std::wstring>& blocked, DWORD excludePid) {
  if (IsBlockedRoot(procs, pid, blocked, excludePid)) return true;
  for (const DWORD ancestor : Ancestors(procs, pid)) {
    if (IsBlockedRoot(procs, ancestor, blocked, excludePid)) return true;
  }
  return false;
}

/**
 * Processos que capturaríamos e quais apps aparecem na lista.
 * A captura inclui a árvore inteira do processo; por isso um processo com um
 * descendente bloqueado (ex.: o explorer.exe que abriu o Discord) fica de fora.
 */
std::unordered_set<DWORD> WantedProcesses(const ProcMap& procs, const std::vector<SessionInfo>& sessions,
                                          const std::vector<std::wstring>& blocked, DWORD excludePid) {
  std::unordered_set<DWORD> tainted;
  for (const auto& [pid, info] : procs) {
    if (IsBlockedRoot(procs, pid, blocked, excludePid)) {
      for (const DWORD ancestor : Ancestors(procs, pid)) tainted.insert(ancestor);
    }
  }
  const DWORD self = GetCurrentProcessId();
  std::unordered_set<DWORD> wanted;
  for (const SessionInfo& session : sessions) {
    if (session.pid == self || tainted.count(session.pid)) continue;
    if (InBlockedTree(procs, session.pid, blocked, excludePid)) continue;
    wanted.insert(session.pid);
  }
  // se um ancestral também vai ser capturado, a árvore dele já inclui este
  for (auto it = wanted.begin(); it != wanted.end();) {
    bool covered = false;
    for (const DWORD ancestor : Ancestors(procs, *it)) {
      if (wanted.count(ancestor)) {
        covered = true;
        break;
      }
    }
    it = covered ? wanted.erase(it) : std::next(it);
  }
  return wanted;
}

}  // namespace

CaptureEngine::CaptureEngine() : wake_(CreateEventW(nullptr, FALSE, FALSE, nullptr)) {}

CaptureEngine::~CaptureEngine() {
  Stop();
  CloseHandle(wake_);
}

void CaptureEngine::Start(std::vector<std::wstring> blocked, DWORD excludePid) {
  Stop();
  {
    std::lock_guard<std::mutex> lock(configMutex_);
    blocked_ = std::move(blocked);
    excludePid_ = excludePid;
  }
  failedAt_.clear();
  running_ = true;
  watcher_ = std::thread(&CaptureEngine::WatchLoop, this);
  mixer_ = std::thread(&CaptureEngine::MixLoop, this);
}

void CaptureEngine::SetBlocked(std::vector<std::wstring> blocked) {
  {
    std::lock_guard<std::mutex> lock(configMutex_);
    blocked_ = std::move(blocked);
  }
  SetEvent(wake_);
}

void CaptureEngine::Stop() {
  if (!running_.exchange(false)) return;
  SetEvent(wake_);
  if (watcher_.joinable()) watcher_.join();
  if (mixer_.joinable()) mixer_.join();
  std::unordered_map<DWORD, std::unique_ptr<ProcessCapture>> captures;
  {
    std::lock_guard<std::mutex> lock(capturesMutex_);
    captures.swap(captures_);
  }
  captures.clear();  // para cada captura fora do lock
}

void CaptureEngine::WatchLoop() {
  CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  while (running_) {
    Refresh();
    WaitForSingleObject(wake_, kRefreshMs);
  }
  CoUninitialize();
}

void CaptureEngine::Refresh() {
  std::vector<std::wstring> blocked;
  DWORD excludePid = 0;
  {
    std::lock_guard<std::mutex> lock(configMutex_);
    blocked = blocked_;
    excludePid = excludePid_;
  }
  const ProcMap procs = SnapshotProcesses();
  const std::unordered_set<DWORD> wanted = WantedProcesses(procs, EnumerateAudioSessions(), blocked, excludePid);

  std::vector<std::unique_ptr<ProcessCapture>> removed;
  {
    std::lock_guard<std::mutex> lock(capturesMutex_);
    for (auto it = captures_.begin(); it != captures_.end();) {
      if (wanted.count(it->first)) {
        ++it;
      } else {
        removed.push_back(std::move(it->second));
        it = captures_.erase(it);
      }
    }
  }
  removed.clear();

  const ULONGLONG now = GetTickCount64();
  for (const DWORD pid : wanted) {
    {
      std::lock_guard<std::mutex> lock(capturesMutex_);
      if (captures_.count(pid)) continue;
    }
    const auto failed = failedAt_.find(pid);
    if (failed != failedAt_.end() && now - failed->second < kRetryFailedMs) continue;

    auto capture = std::make_unique<ProcessCapture>(pid);
    const HRESULT hr = capture->Start();
    if (FAILED(hr)) {
      failedAt_[pid] = now;
      char message[96];
      snprintf(message, sizeof(message), "falha ao capturar o processo %lu (0x%08lx)", pid, static_cast<unsigned long>(hr));
      Log(message);
      continue;
    }
    failedAt_.erase(pid);
    std::lock_guard<std::mutex> lock(capturesMutex_);
    captures_[pid] = std::move(capture);
  }
}

void CaptureEngine::MixLoop() {
  timeBeginPeriod(1);
  LARGE_INTEGER frequency;
  LARGE_INTEGER start;
  QueryPerformanceFrequency(&frequency);
  QueryPerformanceCounter(&start);
  unsigned long long produced = 0;  // quadros já enviados
  std::vector<float> mix(kChunkFrames * kChannels);

  while (running_) {
    LARGE_INTEGER now;
    QueryPerformanceCounter(&now);
    const unsigned long long due =
        static_cast<unsigned long long>((now.QuadPart - start.QuadPart) * kSampleRate / frequency.QuadPart);
    // ficou muito para trás (PC suspenso, travada): pula em vez de despejar tudo
    if (due > produced + kSampleRate / 5) produced = due - kChunkFrames;

    while (produced + kChunkFrames <= due) {
      std::fill(mix.begin(), mix.end(), 0.0f);
      {
        std::lock_guard<std::mutex> lock(capturesMutex_);
        for (auto& [pid, capture] : captures_) capture->MixInto(mix.data(), kChunkFrames);
      }
      for (float& sample : mix) sample = std::clamp(sample, -1.0f, 1.0f);
      SendPcm(mix.data(), kChunkFrames);
      produced += kChunkFrames;
    }
    Sleep(2);
  }
  timeEndPeriod(1);
}

std::string CaptureEngine::AppsJson(const std::vector<std::wstring>& blocked, DWORD excludePid) {
  const ProcMap procs = SnapshotProcesses();
  const DWORD self = GetCurrentProcessId();
  struct Entry {
    bool active = false;
    bool blocked = false;
  };
  std::map<std::wstring, Entry> apps;
  for (const SessionInfo& session : EnumerateAudioSessions()) {
    if (session.pid == self || session.pid == excludePid) continue;
    const auto it = procs.find(session.pid);
    if (it == procs.end()) continue;
    // não mostra o próprio app (ele toca o som das transmissões que você assiste)
    const std::vector<DWORD> ancestors = Ancestors(procs, session.pid);
    if (std::find(ancestors.begin(), ancestors.end(), excludePid) != ancestors.end()) continue;
    Entry& entry = apps[it->second.exe];
    entry.active = entry.active || session.active;
    entry.blocked = entry.blocked || InBlockedTree(procs, session.pid, blocked, excludePid);
  }

  std::string json = "{\"event\":\"apps\",\"apps\":[";
  bool first = true;
  for (const auto& [exe, entry] : apps) {
    const std::string name = ToUtf8(exe);
    json += first ? "" : ",";
    json += "{\"exe\":" + JsonString(name) + ",\"name\":" + JsonString(name.substr(0, name.rfind(".exe"))) +
            ",\"active\":" + (entry.active ? "true" : "false") + ",\"blocked\":" + (entry.blocked ? "true" : "false") +
            "}";
    first = false;
  }
  return json + "]}";
}
