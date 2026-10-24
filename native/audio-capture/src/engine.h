#pragma once

#include <atomic>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_map>

#include "capture.h"
#include "common.h"

/**
 * Decide quais processos capturar e mantém uma captura por processo, mixando
 * tudo em blocos de 10 ms enviados pelo stdout.
 *   - `only` vazio: todos com som, menos os apps bloqueados e o próprio app;
 *   - `only` preenchido: só aqueles apps (e os filhos deles) — "só esta janela".
 */
class CaptureEngine {
 public:
  CaptureEngine();
  ~CaptureEngine();

  void Start(std::vector<std::wstring> blocked, std::vector<std::wstring> only, DWORD excludePid);
  void SetBlocked(std::vector<std::wstring> blocked);
  void SetOnly(std::vector<std::wstring> only);
  void Stop();

  /** Evento "apps": apps com sessão de áudio agora e se estão bloqueados. */
  static std::string AppsJson(const std::vector<std::wstring>& blocked, DWORD excludePid);

 private:
  void WatchLoop();
  void MixLoop();
  void Refresh();

  std::mutex configMutex_;
  std::vector<std::wstring> blocked_;
  /** Não vazio = modo "só este app": só estes executáveis (e os filhos) entram. */
  std::vector<std::wstring> only_;
  DWORD excludePid_ = 0;

  std::mutex capturesMutex_;
  std::unordered_map<DWORD, std::unique_ptr<ProcessCapture>> captures_;
  std::unordered_map<DWORD, ULONGLONG> failedAt_;

  std::atomic<bool> running_{false};
  HANDLE wake_;
  std::thread watcher_;
  std::thread mixer_;
};
