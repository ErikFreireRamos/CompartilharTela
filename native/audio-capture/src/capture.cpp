#include "capture.h"

#include <audioclientactivationparams.h>
#include <avrt.h>
#include <mmdeviceapi.h>

#include <initializer_list>

using Microsoft::WRL::ComPtr;

namespace {

constexpr size_t kRingFrames = kSampleRate;  // 1 s
constexpr size_t kPrimeFrames = 960;         // junta 20 ms antes de começar a entregar
constexpr size_t kMaxBufferedFrames = 4800;  // acima de 100 ms descarta (deriva de relógio)
constexpr REFERENCE_TIME kBufferDuration = 200000;  // 20 ms em unidades de 100 ns

// ActivateAudioInterfaceAsync exige um handler "agile" (IAgileObject).
class ActivationHandler final : public IActivateAudioInterfaceCompletionHandler, public IAgileObject {
 public:
  ActivationHandler() : done_(CreateEventW(nullptr, TRUE, FALSE, nullptr)) {}
  ~ActivationHandler() { CloseHandle(done_); }

  ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&refs_); }
  ULONG STDMETHODCALLTYPE Release() override {
    const ULONG refs = InterlockedDecrement(&refs_);
    if (refs == 0) delete this;
    return refs;
  }
  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** out) override {
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IActivateAudioInterfaceCompletionHandler)) {
      *out = static_cast<IActivateAudioInterfaceCompletionHandler*>(this);
    } else if (riid == __uuidof(IAgileObject)) {
      *out = static_cast<IAgileObject*>(this);
    } else {
      *out = nullptr;
      return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
  }

  HRESULT STDMETHODCALLTYPE ActivateCompleted(IActivateAudioInterfaceAsyncOperation* operation) override {
    HRESULT activateResult = E_FAIL;
    ComPtr<IUnknown> unknown;
    HRESULT hr = operation->GetActivateResult(&activateResult, &unknown);
    if (SUCCEEDED(hr)) hr = activateResult;
    if (SUCCEEDED(hr)) hr = unknown.As(&client);
    result = hr;
    SetEvent(done_);
    return S_OK;
  }

  bool Wait(DWORD ms) { return WaitForSingleObject(done_, ms) == WAIT_OBJECT_0; }

  HRESULT result = E_FAIL;
  ComPtr<IAudioClient> client;

 private:
  LONG refs_ = 1;
  HANDLE done_;
};

HRESULT ActivateProcessLoopback(DWORD pid, ComPtr<IAudioClient>& client) {
  AUDIOCLIENT_ACTIVATION_PARAMS params{};
  params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
  params.ProcessLoopbackParams.TargetProcessId = pid;
  params.ProcessLoopbackParams.ProcessLoopbackMode = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;

  PROPVARIANT prop{};
  prop.vt = VT_BLOB;
  prop.blob.cbSize = sizeof(params);
  prop.blob.pBlobData = reinterpret_cast<BYTE*>(&params);

  ComPtr<ActivationHandler> handler;
  handler.Attach(new ActivationHandler());
  ComPtr<IActivateAudioInterfaceAsyncOperation> operation;
  HRESULT hr = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, __uuidof(IAudioClient), &prop,
                                           handler.Get(), &operation);
  if (FAILED(hr)) return hr;
  if (!handler->Wait(5000)) return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
  if (FAILED(handler->result)) return handler->result;
  client = handler->client;
  return S_OK;
}

WAVEFORMATEX MakeFormat(bool isFloat) {
  WAVEFORMATEX format{};
  format.wFormatTag = isFloat ? WAVE_FORMAT_IEEE_FLOAT : WAVE_FORMAT_PCM;
  format.nChannels = kChannels;
  format.nSamplesPerSec = kSampleRate;
  format.wBitsPerSample = isFloat ? 32 : 16;
  format.nBlockAlign = static_cast<WORD>(format.nChannels * format.wBitsPerSample / 8);
  format.nAvgBytesPerSec = format.nSamplesPerSec * format.nBlockAlign;
  return format;
}

// Process loopback não informa o formato do mix: pedimos 48 kHz estéreo e o
// Windows converte (AUTOCONVERTPCM). Float primeiro; PCM 16 bits se recusar.
HRESULT InitializeClient(DWORD pid, ComPtr<IAudioClient>& client, bool& isFloat) {
  HRESULT hr = E_FAIL;
  for (const bool tryFloat : {true, false}) {
    client.Reset();
    hr = ActivateProcessLoopback(pid, client);
    if (FAILED(hr)) return hr;
    WAVEFORMATEX format = MakeFormat(tryFloat);
    hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED,
                            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                            kBufferDuration, 0, &format, nullptr);
    if (SUCCEEDED(hr)) {
      isFloat = tryFloat;
      return hr;
    }
  }
  return hr;
}

}  // namespace

bool ProcessLoopbackSupported() {
  ComPtr<IAudioClient> client;
  bool isFloat = true;
  return SUCCEEDED(InitializeClient(GetCurrentProcessId(), client, isFloat));
}

HRESULT ProcessCapture::Start() {
  HRESULT hr = InitializeClient(pid_, client_, isFloat_);
  if (FAILED(hr)) return hr;
  if (FAILED(hr = client_->GetService(IID_PPV_ARGS(&capture_)))) return hr;
  event_ = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  if (FAILED(hr = client_->SetEventHandle(event_))) return hr;
  ring_.assign(kRingFrames * kChannels, 0.0f);
  if (FAILED(hr = client_->Start())) return hr;
  thread_ = std::thread(&ProcessCapture::Run, this);
  return S_OK;
}

void ProcessCapture::Stop() {
  stop_ = true;
  if (thread_.joinable()) thread_.join();
  if (client_) client_->Stop();
  capture_.Reset();
  client_.Reset();
  if (event_) {
    CloseHandle(event_);
    event_ = nullptr;
  }
}

void ProcessCapture::Run() {
  CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  DWORD taskIndex = 0;
  HANDLE task = AvSetMmThreadCharacteristicsW(L"Audio", &taskIndex);
  while (!stop_) {
    if (WaitForSingleObject(event_, 100) != WAIT_OBJECT_0) continue;
    UINT32 packet = 0;
    while (!stop_ && SUCCEEDED(capture_->GetNextPacketSize(&packet)) && packet > 0) {
      BYTE* data = nullptr;
      UINT32 frames = 0;
      DWORD flags = 0;
      if (FAILED(capture_->GetBuffer(&data, &frames, &flags, nullptr, nullptr))) break;
      Push(data, frames, (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0);
      capture_->ReleaseBuffer(frames);
    }
  }
  if (task) AvRevertMmThreadCharacteristics(task);
  CoUninitialize();
}

void ProcessCapture::Push(const BYTE* data, UINT32 frames, bool silent) {
  std::lock_guard<std::mutex> lock(mutex_);
  for (UINT32 i = 0; i < frames; ++i) {
    float left = 0.0f;
    float right = 0.0f;
    if (!silent) {
      if (isFloat_) {
        const float* sample = reinterpret_cast<const float*>(data) + i * 2;
        left = sample[0];
        right = sample[1];
      } else {
        const int16_t* sample = reinterpret_cast<const int16_t*>(data) + i * 2;
        left = sample[0] / 32768.0f;
        right = sample[1] / 32768.0f;
      }
    }
    ring_[write_ * 2] = left;
    ring_[write_ * 2 + 1] = right;
    write_ = (write_ + 1) % kRingFrames;
    if (count_ < kRingFrames) {
      ++count_;
    } else {
      read_ = (read_ + 1) % kRingFrames;  // cheio: descarta o mais antigo
    }
  }
}

bool ProcessCapture::MixInto(float* out, size_t frames) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (count_ > kMaxBufferedFrames) {
    const size_t drop = count_ - kPrimeFrames;
    read_ = (read_ + drop) % kRingFrames;
    count_ -= drop;
  }
  if (!primed_) {
    if (count_ < kPrimeFrames) return false;
    primed_ = true;
  }
  if (count_ < frames) {
    primed_ = false;
    return false;
  }
  for (size_t i = 0; i < frames; ++i) {
    out[i * 2] += ring_[read_ * 2];
    out[i * 2 + 1] += ring_[read_ * 2 + 1];
    read_ = (read_ + 1) % kRingFrames;
  }
  count_ -= frames;
  return true;
}
