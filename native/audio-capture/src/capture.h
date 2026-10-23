#pragma once

#include <audioclient.h>
#include <wrl/client.h>

#include <atomic>
#include <mutex>
#include <thread>

#include "common.h"

/**
 * Captura o áudio que um processo (e os filhos dele) toca, via WASAPI process
 * loopback (Windows 10 2004+ / 11). Guarda até 1 s num buffer circular que o
 * mixer consome em blocos de 10 ms.
 */
class ProcessCapture {
 public:
  explicit ProcessCapture(DWORD pid) : pid_(pid) {}
  ~ProcessCapture() { Stop(); }
  ProcessCapture(const ProcessCapture&) = delete;
  ProcessCapture& operator=(const ProcessCapture&) = delete;

  HRESULT Start();
  void Stop();

  /** Soma `frames` quadros estéreo em `out`. false se ainda não há dados suficientes. */
  bool MixInto(float* out, size_t frames);

 private:
  void Run();
  void Push(const BYTE* data, UINT32 frames, bool silent);

  const DWORD pid_;
  bool isFloat_ = true;
  Microsoft::WRL::ComPtr<IAudioClient> client_;
  Microsoft::WRL::ComPtr<IAudioCaptureClient> capture_;
  HANDLE event_ = nullptr;
  std::thread thread_;
  std::atomic<bool> stop_{false};

  std::mutex mutex_;
  std::vector<float> ring_;  // estéreo intercalado
  size_t read_ = 0;          // posições e contagem em quadros
  size_t write_ = 0;
  size_t count_ = 0;
  bool primed_ = false;
};

/** true se este Windows suporta captura de áudio por processo. */
bool ProcessLoopbackSupported();
