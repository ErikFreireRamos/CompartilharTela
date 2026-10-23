#pragma once

#include <unordered_map>

#include "common.h"

struct ProcInfo {
  DWORD parent = 0;
  std::wstring exe;  // em minúsculas, ex.: "discord.exe"
};
using ProcMap = std::unordered_map<DWORD, ProcInfo>;

ProcMap SnapshotProcesses();

/** Pais, avós... do processo (mais próximo primeiro). */
std::vector<DWORD> Ancestors(const ProcMap& procs, DWORD pid);

struct SessionInfo {
  DWORD pid;
  bool active;  // tocando som agora
};

/** Sessões de áudio de todas as saídas de som ativas (sem os sons do sistema). */
std::vector<SessionInfo> EnumerateAudioSessions();
