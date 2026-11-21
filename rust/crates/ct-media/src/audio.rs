//! Captura de áudio real no Windows (WASAPI Process Loopback por app com bloqueio ao vivo
//! + fallback automático para WASAPI Loopback do sistema) e mixer de reprodução WASAPI
//! (`IAudioRenderClient`) com controle de volume por participante (0%–200%).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: usize = 2;
/// 20 ms por bloco @ 48 kHz estéreo = 960 frames = 1920 amostras f32.
pub const AUDIO_CHUNK_FRAMES: usize = 960;
pub const AUDIO_CHUNK_SAMPLES: usize = AUDIO_CHUNK_FRAMES * AUDIO_CHANNELS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioApp {
    pub exe: String,
    pub name: String,
    pub active: bool,
    pub blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    Native,
    System,
    None,
}

/// Aplica ganho inline com soft-clipping (suporta até 200% sem estourar).
#[inline]
pub fn apply_gain_slice(samples: &mut [f32], gain: f32) {
    for s in samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

// ============================================================================
// Captura de Áudio (WASAPI Process Loopback + Fallback Sistema)
// ============================================================================

pub struct AudioCaptureEngine {
    running: Arc<AtomicBool>,
    send_volume_bits: Arc<AtomicU32>,
    blocked: Arc<(StdMutex<HashSet<String>>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl Default for AudioCaptureEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCaptureEngine {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            send_volume_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            blocked: Arc::new((StdMutex::new(HashSet::new()), Condvar::new())),
            worker: None,
        }
    }

    pub fn set_send_volume(&self, volume: f32) {
        self.send_volume_bits
            .store(volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn send_volume(&self) -> f32 {
        f32::from_bits(self.send_volume_bits.load(Ordering::Relaxed))
    }

    /// Atualiza a lista de apps bloqueados ao vivo (acorda o watcher imediatamente).
    pub fn set_blocked_sync(&self, apps: &[String]) {
        let (lock, cvar) = &*self.blocked;
        if let Ok(mut guard) = lock.lock() {
            guard.clear();
            for app in apps {
                guard.insert(app.to_ascii_lowercase());
            }
            cvar.notify_all();
        }
    }

    pub async fn set_blocked(&self, apps: &[String]) {
        self.set_blocked_sync(apps);
    }

    pub fn list_apps(&self, blocked: &[String]) -> Vec<AudioApp> {
        let blocked_set: HashSet<String> = blocked.iter().map(|s| s.to_ascii_lowercase()).collect();
        enumerate_audio_apps(&blocked_set)
    }

    /// Inicia a captura e retorna `(AudioMode, Receiver<Vec<f32>>)` com blocos de 20 ms @ 48 kHz estéreo.
    pub fn start(&mut self, blocked: &[String]) -> Result<(AudioMode, mpsc::Receiver<Vec<f32>>), String> {
        self.stop();
        self.set_blocked_sync(blocked);

        let (tx, rx) = mpsc::channel::<Vec<f32>>(16);
        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let vol_bits = self.send_volume_bits.clone();
        let blocked_ref = self.blocked.clone();

        #[cfg(windows)]
        let mode = if win_audio::process_loopback_supported() {
            AudioMode::Native
        } else {
            AudioMode::System
        };
        #[cfg(not(windows))]
        let mode = AudioMode::Native;

        let handle = thread::Builder::new()
            .name("ct-audio-capture".to_string())
            .spawn(move || {
                #[cfg(windows)]
                {
                    if mode == AudioMode::Native {
                        win_audio::run_per_app_capture_loop(running, vol_bits, blocked_ref, tx);
                    } else {
                        win_audio::run_system_loopback_loop(running, vol_bits, tx);
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = blocked_ref;
                    run_dummy_capture_loop(running, vol_bits, tx);
                }
            })
            .map_err(|e| format!("Falha ao iniciar thread de áudio: {e}"))?;

        self.worker = Some(handle);
        Ok((mode, rx))
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let (_, cvar) = &*self.blocked;
        cvar.notify_all();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioCaptureEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(windows))]
fn run_dummy_capture_loop(
    running: Arc<AtomicBool>,
    send_volume_bits: Arc<AtomicU32>,
    tx: mpsc::Sender<Vec<f32>>,
) {
    while running.load(Ordering::Relaxed) {
        let gain = f32::from_bits(send_volume_bits.load(Ordering::Relaxed));
        let mut buf = vec![0.0f32; AUDIO_CHUNK_SAMPLES];
        apply_gain_slice(&mut buf, gain);
        let _ = tx.try_send(buf);
        thread::sleep(Duration::from_millis(20));
    }
}

// ============================================================================
// Reprodução de Áudio Remoto (Mixer Multi-Stream WASAPI)
// ============================================================================

struct PeerAudioQueue {
    samples: VecDeque<f32>,
    volume: f32,
}

struct PlaybackShared {
    peers: StdMutex<HashMap<String, PeerAudioQueue>>,
}

/// Reproduz áudio recebido de todos os participantes remotos via WASAPI `IAudioRenderClient`,
/// aplicando o volume individual de cada participante (0%–200%).
pub struct AudioPlaybackMixer {
    running: Arc<AtomicBool>,
    shared: Arc<PlaybackShared>,
    worker: Option<JoinHandle<()>>,
}

impl Default for AudioPlaybackMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlaybackMixer {
    pub fn new() -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let shared = Arc::new(PlaybackShared {
            peers: StdMutex::new(HashMap::new()),
        });

        #[cfg(windows)]
        let worker = {
            let run_flag = running.clone();
            let sh = shared.clone();
            thread::Builder::new()
                .name("ct-audio-playback".to_string())
                .spawn(move || {
                    win_audio::run_playback_loop(run_flag, sh);
                })
                .ok()
        };
        #[cfg(not(windows))]
        let worker = None;

        Self {
            running,
            shared,
            worker,
        }
    }

    pub fn push_samples(&self, identity: &str, samples: &[f32], volume: f32) {
        const MAX_BUFFERED_SAMPLES: usize = (AUDIO_SAMPLE_RATE as usize / 6) * AUDIO_CHANNELS; // ~166 ms
        if let Ok(mut map) = self.shared.peers.lock() {
            let entry = map
                .entry(identity.to_string())
                .or_insert_with(|| PeerAudioQueue {
                    samples: VecDeque::with_capacity(MAX_BUFFERED_SAMPLES),
                    volume,
                });
            entry.volume = volume.clamp(0.0, 2.0);
            entry.samples.extend(samples.iter().copied());
            if entry.samples.len() > MAX_BUFFERED_SAMPLES {
                let drop_count = entry.samples.len() - (AUDIO_CHUNK_SAMPLES * 2);
                entry.samples.drain(..drop_count);
            }
        }
    }

    pub fn set_peer_volume(&self, identity: &str, volume: f32) {
        if let Ok(mut map) = self.shared.peers.lock()
            && let Some(entry) = map.get_mut(identity)
        {
            entry.volume = volume.clamp(0.0, 2.0);
        }
    }

    pub fn remove_peer(&self, identity: &str) {
        if let Ok(mut map) = self.shared.peers.lock() {
            map.remove(identity);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.shared.peers.lock() {
            map.clear();
        }
    }
}

impl Drop for AudioPlaybackMixer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// Enumeração de Apps com Áudio
// ============================================================================

#[cfg(not(windows))]
fn enumerate_audio_apps(blocked: &HashSet<String>) -> Vec<AudioApp> {
    blocked
        .iter()
        .map(|exe| AudioApp {
            exe: exe.clone(),
            name: exe.strip_suffix(".exe").unwrap_or(exe).to_string(),
            active: false,
            blocked: true,
        })
        .collect()
}

#[cfg(windows)]
fn enumerate_audio_apps(blocked: &HashSet<String>) -> Vec<AudioApp> {
    win_audio::enumerate_audio_apps_win(blocked)
}

// ============================================================================
// Implementação Windows WASAPI (Process Loopback + System Loopback + Render)
// ============================================================================

#[cfg(windows)]
mod win_audio {
    use super::*;
    use std::collections::BTreeMap;
    use std::mem::{ManuallyDrop, size_of};
    use std::time::Instant;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
        AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
        ActivateAudioInterfaceAsync, AudioSessionStateActive, IActivateAudioInterfaceAsyncOperation,
        IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
        IAudioCaptureClient, IAudioClient, IAudioRenderClient, IAudioSessionControl2,
        IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        WAVE_FORMAT_PCM, WAVEFORMATEX, eConsole, eRender,
    };
    use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::{
        BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        IAgileObject, IAgileObject_Impl,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, OpenProcess,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        SetEvent, WaitForSingleObject,
    };
    use windows::Win32::System::Variant::VT_BLOB;
    use windows::core::{HRESULT, Interface, PWSTR, Ref, Result as WinResult, implement};

    #[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
    struct ActivationHandler {
        event: HANDLE,
        state: StdMutex<Option<WinResult<IAudioClient>>>,
    }

    impl IAgileObject_Impl for ActivationHandler_Impl {}

    impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
        fn ActivateCompleted(
            &self,
            activateoperation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
        ) -> WinResult<()> {
            let res = (|| -> WinResult<IAudioClient> {
                let op = activateoperation.ok()?;
                let mut hr = HRESULT(0);
                let mut unknown = None;
                unsafe {
                    op.GetActivateResult(&mut hr, &mut unknown)?;
                }
                hr.ok()?;
                let unk = unknown.ok_or_else(|| windows::core::Error::from(HRESULT(-1)))?;
                unk.cast::<IAudioClient>()
            })();
            if let Ok(mut guard) = self.state.lock() {
                *guard = Some(res);
            }
            unsafe {
                let _ = SetEvent(self.event);
            }
            Ok(())
        }
    }

    fn make_wave_format(is_float: bool) -> WAVEFORMATEX {
        let bits: u16 = if is_float { 32 } else { 16 };
        let block_align = (AUDIO_CHANNELS as u16) * (bits / 8);
        WAVEFORMATEX {
            wFormatTag: if is_float {
                WAVE_FORMAT_IEEE_FLOAT as u16
            } else {
                WAVE_FORMAT_PCM as u16
            },
            nChannels: AUDIO_CHANNELS as u16,
            nSamplesPerSec: AUDIO_SAMPLE_RATE,
            nAvgBytesPerSec: AUDIO_SAMPLE_RATE * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: bits,
            cbSize: 0,
        }
    }

    fn activate_process_loopback(pid: u32) -> WinResult<IAudioClient> {
        unsafe {
            let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
                ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
                Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                    ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                        TargetProcessId: pid,
                        ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                    },
                },
            };

            let prop = PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_BLOB,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 {
                            blob: BLOB {
                                cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                                pBlobData: (&mut params as *mut AUDIOCLIENT_ACTIVATION_PARAMS)
                                    as *mut u8,
                            },
                        },
                    }),
                },
            };

            let event = CreateEventW(None, true, false, None)?;
            let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler {
                event,
                state: StdMutex::new(None),
            }
            .into();

            let _op = ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(&prop),
                &handler,
            )?;

            let wait = WaitForSingleObject(event, 3000);
            let _ = CloseHandle(event);
            if wait != WAIT_OBJECT_0 {
                return Err(windows::core::Error::from(HRESULT(-1)));
            }
            let impl_ref: &ActivationHandler =
                windows::core::AsImpl::as_impl(&handler);
            let out = impl_ref
                .state
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_else(|| Err(windows::core::Error::from(HRESULT(-1))));
            out
        }
    }

    fn init_process_client(pid: u32) -> WinResult<(IAudioClient, bool)> {
        const BUFFER_DURATION_100NS: i64 = 200_000; // 20 ms
        for is_float in [true, false] {
            let client = activate_process_loopback(pid)?;
            let fmt = make_wave_format(is_float);
            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;
            unsafe {
                if client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        flags,
                        BUFFER_DURATION_100NS,
                        0,
                        &fmt,
                        None,
                    )
                    .is_ok()
                {
                    return Ok((client, is_float));
                }
            }
        }
        Err(windows::core::Error::from(HRESULT(-1)))
    }

    pub fn process_loopback_supported() -> bool {
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let ok = init_process_client(std::process::id()).is_ok();
            if com {
                CoUninitialize();
            }
            ok
        }
    }

    struct ProcInfo {
        parent_pid: u32,
        exe: String,
    }

    fn snapshot_processes() -> HashMap<u32, ProcInfo> {
        let mut map = HashMap::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return map;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                    map.insert(
                        entry.th32ProcessID,
                        ProcInfo {
                            parent_pid: entry.th32ParentProcessID,
                            exe,
                        },
                    );
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        map
    }

    fn ancestors(procs: &HashMap<u32, ProcInfo>, mut pid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        while let Some(info) = procs.get(&pid) {
            let parent = info.parent_pid;
            if parent == 0 || parent == pid || !visited.insert(parent) {
                break;
            }
            out.push(parent);
            pid = parent;
        }
        out
    }

    struct SessionEntry {
        pid: u32,
        active: bool,
    }

    fn enumerate_audio_sessions() -> Vec<SessionEntry> {
        let mut out = Vec::new();
        unsafe {
            let res = (|| -> WinResult<()> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
                let session_enum = mgr.GetSessionEnumerator()?;
                let count = session_enum.GetCount()?;
                for i in 0..count {
                    let Ok(ctrl) = session_enum.GetSession(i) else {
                        continue;
                    };
                    let active = ctrl.GetState().ok() == Some(AudioSessionStateActive);
                    let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
                        continue;
                    };
                    let Ok(pid) = ctrl2.GetProcessId() else {
                        continue;
                    };
                    if pid != 0 {
                        out.push(SessionEntry { pid, active });
                    }
                }
                Ok(())
            })();
            let _ = res;
        }
        out
    }

    fn query_exe_name(pid: u32) -> Option<String> {
        unsafe {
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                proc,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(proc);
            if !ok {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            std::path::Path::new(&full)
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
        }
    }

    pub fn enumerate_audio_apps_win(blocked: &HashSet<String>) -> Vec<AudioApp> {
        let mut apps: BTreeMap<String, AudioApp> = BTreeMap::new();
        let self_pid = std::process::id();
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let procs = snapshot_processes();
            for s in enumerate_audio_sessions() {
                if s.pid == self_pid {
                    continue;
                }
                let exe = procs
                    .get(&s.pid)
                    .map(|p| p.exe.clone())
                    .or_else(|| query_exe_name(s.pid))
                    .unwrap_or_default();
                if exe.is_empty() {
                    continue;
                }
                let display = exe.strip_suffix(".exe").unwrap_or(&exe).to_string();
                let is_blocked = blocked.contains(&exe);
                let entry = apps.entry(exe.clone()).or_insert(AudioApp {
                    exe,
                    name: display,
                    active: false,
                    blocked: is_blocked,
                });
                entry.active = entry.active || s.active;
                entry.blocked = entry.blocked || is_blocked;
            }
            if com {
                CoUninitialize();
            }
        }
        apps.into_values().collect()
    }

    struct ProcessCaptureHandle {
        stop: Arc<AtomicBool>,
        ring: Arc<StdMutex<VecDeque<f32>>>,
        thread: Option<JoinHandle<()>>,
    }

    impl ProcessCaptureHandle {
        fn start(pid: u32) -> Result<Self, ()> {
            let stop = Arc::new(AtomicBool::new(false));
            let ring = Arc::new(StdMutex::new(VecDeque::with_capacity(
                AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS / 4,
            )));
            let stop_c = stop.clone();
            let ring_c = ring.clone();

            // Verifica inicialização na própria thread COM do capturador
            let (init_tx, init_rx) = std::sync::mpsc::channel::<bool>();
            let handle = thread::Builder::new()
                .name(format!("ct-wasapi-pid-{pid}"))
                .spawn(move || unsafe {
                    let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
                    let Ok((client, is_float)) = init_process_client(pid) else {
                        let _ = init_tx.send(false);
                        if com {
                            CoUninitialize();
                        }
                        return;
                    };
                    let Ok(capture) = client.GetService::<IAudioCaptureClient>() else {
                        let _ = init_tx.send(false);
                        if com {
                            CoUninitialize();
                        }
                        return;
                    };
                    let Ok(event) = CreateEventW(None, false, false, None) else {
                        let _ = init_tx.send(false);
                        if com {
                            CoUninitialize();
                        }
                        return;
                    };
                    if client.SetEventHandle(event).is_err() || client.Start().is_err() {
                        let _ = CloseHandle(event);
                        let _ = init_tx.send(false);
                        if com {
                            CoUninitialize();
                        }
                        return;
                    }
                    let _ = init_tx.send(true);

                    let mut task_idx = 0u32;
                    let task = AvSetMmThreadCharacteristicsW(windows::core::w!("Audio"), &mut task_idx).ok();

                    while !stop_c.load(Ordering::Relaxed) {
                        if WaitForSingleObject(event, 80) != WAIT_OBJECT_0 {
                            continue;
                        }
                        loop {
                            let Ok(packet) = capture.GetNextPacketSize() else {
                                break;
                            };
                            if packet == 0 || stop_c.load(Ordering::Relaxed) {
                                break;
                            }
                            let mut data_ptr = std::ptr::null_mut();
                            let mut frames = 0u32;
                            let mut flags = 0u32;
                            if capture
                                .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                                .is_err()
                            {
                                break;
                            }
                            let silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0;
                            let total_samples = (frames as usize) * AUDIO_CHANNELS;
                            if let Ok(mut q) = ring_c.lock() {
                                if silent || data_ptr.is_null() {
                                    for _ in 0..total_samples {
                                        q.push_back(0.0);
                                    }
                                } else if is_float {
                                    let sl = std::slice::from_raw_parts(
                                        data_ptr as *const f32,
                                        total_samples,
                                    );
                                    q.extend(sl.iter().copied());
                                } else {
                                    let sl = std::slice::from_raw_parts(
                                        data_ptr as *const i16,
                                        total_samples,
                                    );
                                    q.extend(sl.iter().map(|&s| f32::from(s) / 32768.0));
                                }
                                let max_q = (AUDIO_SAMPLE_RATE as usize / 6) * AUDIO_CHANNELS;
                                if q.len() > max_q {
                                    let drop_n = q.len() - (AUDIO_CHUNK_SAMPLES * 2);
                                    q.drain(..drop_n);
                                }
                            }
                            let _ = capture.ReleaseBuffer(frames);
                        }
                    }

                    let _ = client.Stop();
                    if let Some(t) = task {
                        let _ = AvRevertMmThreadCharacteristics(t);
                    }
                    let _ = CloseHandle(event);
                    if com {
                        CoUninitialize();
                    }
                })
                .map_err(|_| ())?;

            if init_rx.recv().unwrap_or(false) {
                Ok(Self {
                    stop,
                    ring,
                    thread: Some(handle),
                })
            } else {
                let _ = handle.join();
                Err(())
            }
        }

        fn mix_into(&self, out: &mut [f32]) {
            if let Ok(mut q) = self.ring.lock() {
                if q.len() < out.len() {
                    return;
                }
                for slot in out.iter_mut() {
                    if let Some(v) = q.pop_front() {
                        *slot += v;
                    }
                }
            }
        }
    }

    impl Drop for ProcessCaptureHandle {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn wanted_pids(
        procs: &HashMap<u32, ProcInfo>,
        sessions: &[SessionEntry],
        blocked: &HashSet<String>,
        self_pid: u32,
    ) -> HashSet<u32> {
        let is_blocked_root = |pid: u32| -> bool {
            pid == self_pid
                || procs
                    .get(&pid)
                    .is_some_and(|p| blocked.contains(&p.exe))
        };
        let in_blocked_tree = |pid: u32| -> bool {
            if is_blocked_root(pid) {
                return true;
            }
            ancestors(procs, pid).into_iter().any(is_blocked_root)
        };

        let mut tainted = HashSet::new();
        for &pid in procs.keys() {
            if is_blocked_root(pid) {
                for anc in ancestors(procs, pid) {
                    tainted.insert(anc);
                }
            }
        }

        let mut wanted = HashSet::new();
        for s in sessions {
            if s.pid == self_pid || tainted.contains(&s.pid) || in_blocked_tree(s.pid) {
                continue;
            }
            wanted.insert(s.pid);
        }

        let snapshot_wanted = wanted.clone();
        wanted.retain(|&pid| {
            !ancestors(procs, pid)
                .into_iter()
                .any(|anc| snapshot_wanted.contains(&anc))
        });
        wanted
    }

    pub fn run_per_app_capture_loop(
        running: Arc<AtomicBool>,
        send_volume_bits: Arc<AtomicU32>,
        blocked: Arc<(StdMutex<HashSet<String>>, Condvar)>,
        tx: mpsc::Sender<Vec<f32>>,
    ) {
        let captures: Arc<StdMutex<HashMap<u32, ProcessCaptureHandle>>> =
            Arc::new(StdMutex::new(HashMap::new()));

        let run_watch = running.clone();
        let cap_watch = captures.clone();
        let blocked_watch = blocked.clone();

        let watcher = thread::Builder::new()
            .name("ct-audio-watcher".to_string())
            .spawn(move || unsafe {
                let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
                let self_pid = std::process::id();
                let mut failed_at: HashMap<u32, Instant> = HashMap::new();

                while run_watch.load(Ordering::Relaxed) {
                    let blocked_set = {
                        let (lock, _) = &*blocked_watch;
                        lock.lock().map(|g| g.clone()).unwrap_or_default()
                    };
                    let procs = snapshot_processes();
                    let sessions = enumerate_audio_sessions();
                    let wanted = wanted_pids(&procs, &sessions, &blocked_set, self_pid);

                    // Remove bloqueados ou encerrados
                    let removed: Vec<ProcessCaptureHandle> = {
                        let mut map = cap_watch.lock().unwrap();
                        let to_remove: Vec<u32> = map
                            .keys()
                            .copied()
                            .filter(|pid| !wanted.contains(pid))
                            .collect();
                        to_remove
                            .into_iter()
                            .filter_map(|pid| map.remove(&pid))
                            .collect()
                    };
                    drop(removed);

                    // Inicia novos processos desejados
                    let now = Instant::now();
                    for pid in wanted {
                        if cap_watch.lock().unwrap().contains_key(&pid) {
                            continue;
                        }
                        if failed_at
                            .get(&pid)
                            .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(10))
                        {
                            continue;
                        }
                        match ProcessCaptureHandle::start(pid) {
                            Ok(h) => {
                                failed_at.remove(&pid);
                                cap_watch.lock().unwrap().insert(pid, h);
                            }
                            Err(()) => {
                                failed_at.insert(pid, now);
                            }
                        }
                    }

                    // Dorme 1s ou acorda imediatamente quando blocked muda
                    let (lock, cvar) = &*blocked_watch;
                    if let Ok(guard) = lock.lock() {
                        let _ = cvar.wait_timeout(guard, Duration::from_millis(800));
                    }
                }

                if let Ok(mut map) = cap_watch.lock() {
                    map.clear();
                }
                if com {
                    CoUninitialize();
                }
            })
            .ok();

        // Mixer de 20 ms sincronizado pelo relógio monotônico
        let start = Instant::now();
        let mut produced_chunks: u64 = 0;

        while running.load(Ordering::Relaxed) {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let due_chunks = elapsed_ms / 20;
            if due_chunks > produced_chunks + 10 {
                produced_chunks = due_chunks.saturating_sub(1);
            }
            while produced_chunks < due_chunks && running.load(Ordering::Relaxed) {
                let mut mix = vec![0.0f32; AUDIO_CHUNK_SAMPLES];
                if let Ok(map) = captures.lock() {
                    for handle in map.values() {
                        handle.mix_into(&mut mix);
                    }
                }
                let gain = f32::from_bits(send_volume_bits.load(Ordering::Relaxed));
                apply_gain_slice(&mut mix, gain);
                let _ = tx.try_send(mix);
                produced_chunks += 1;
            }
            thread::sleep(Duration::from_millis(4));
        }

        if let Some(w) = watcher {
            let _ = w.join();
        }
    }

    pub fn run_system_loopback_loop(
        running: Arc<AtomicBool>,
        send_volume_bits: Arc<AtomicU32>,
        tx: mpsc::Sender<Vec<f32>>,
    ) {
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let _ = (|| -> WinResult<()> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                let fmt = make_wave_format(true);
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                    200_000,
                    0,
                    &fmt,
                    None,
                )?;
                let capture: IAudioCaptureClient = client.GetService()?;
                client.Start()?;

                let mut accum = Vec::<f32>::with_capacity(AUDIO_CHUNK_SAMPLES * 2);
                while running.load(Ordering::Relaxed) {
                    loop {
                        let Ok(packet) = capture.GetNextPacketSize() else {
                            break;
                        };
                        if packet == 0 {
                            break;
                        }
                        let mut data_ptr = std::ptr::null_mut();
                        let mut frames = 0u32;
                        let mut flags = 0u32;
                        if capture
                            .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                            .is_err()
                        {
                            break;
                        }
                        let silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0;
                        let total = (frames as usize) * AUDIO_CHANNELS;
                        if silent || data_ptr.is_null() {
                            accum.resize(accum.len() + total, 0.0);
                        } else {
                            let sl = std::slice::from_raw_parts(data_ptr as *const f32, total);
                            accum.extend_from_slice(sl);
                        }
                        let _ = capture.ReleaseBuffer(frames);

                        while accum.len() >= AUDIO_CHUNK_SAMPLES {
                            let mut chunk: Vec<f32> = accum.drain(..AUDIO_CHUNK_SAMPLES).collect();
                            let gain = f32::from_bits(send_volume_bits.load(Ordering::Relaxed));
                            apply_gain_slice(&mut chunk, gain);
                            let _ = tx.try_send(chunk);
                        }
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                let _ = client.Stop();
                Ok(())
            })();
            if com {
                CoUninitialize();
            }
        }
    }

    pub fn run_playback_loop(running: Arc<AtomicBool>, shared: Arc<PlaybackShared>) {
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let _ = (|| -> WinResult<()> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                let fmt = make_wave_format(true);
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                    400_000, // 40 ms buffer
                    0,
                    &fmt,
                    None,
                )?;
                let buf_frames = client.GetBufferSize()?;
                let render: IAudioRenderClient = client.GetService()?;
                client.Start()?;

                let mut task_idx = 0u32;
                let task =
                    AvSetMmThreadCharacteristicsW(windows::core::w!("Audio"), &mut task_idx).ok();

                while running.load(Ordering::Relaxed) {
                    let Ok(padding) = client.GetCurrentPadding() else {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    let avail = buf_frames.saturating_sub(padding);
                    if avail >= 240 {
                        if let Ok(ptr) = render.GetBuffer(avail) {
                            let total = (avail as usize) * AUDIO_CHANNELS;
                            let out = std::slice::from_raw_parts_mut(ptr as *mut f32, total);
                            out.fill(0.0);

                            if let Ok(mut map) = shared.peers.lock() {
                                for peer in map.values_mut() {
                                    let vol = peer.volume;
                                    if peer.samples.is_empty() {
                                        continue;
                                    }
                                    let take = total.min(peer.samples.len());
                                    for slot in out.iter_mut().take(take) {
                                        if let Some(s) = peer.samples.pop_front() {
                                            *slot += s * vol;
                                        }
                                    }
                                }
                            }
                            for s in out.iter_mut() {
                                *s = s.clamp(-1.0, 1.0);
                            }
                            let _ = render.ReleaseBuffer(avail, 0);
                        }
                    }
                    thread::sleep(Duration::from_millis(5));
                }

                let _ = client.Stop();
                if let Some(t) = task {
                    let _ = AvRevertMmThreadCharacteristics(t);
                }
                Ok(())
            })();
            if com {
                CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn audio_capture_emits_20ms_chunks() {
        let mut engine = AudioCaptureEngine::new();
        engine.set_send_volume(1.5);
        assert!((engine.send_volume() - 1.5).abs() < f32::EPSILON);

        let (_mode, mut rx) = engine.start(&["discord.exe".into()]).unwrap();
        let chunk = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        engine.stop();
        assert_eq!(chunk.len(), AUDIO_CHUNK_SAMPLES);
    }

    #[test]
    fn gain_clamps_to_unit_range() {
        let mut samples = [0.8, -0.8, 0.25];
        apply_gain_slice(&mut samples, 2.0);
        assert_eq!(samples, [1.0, -1.0, 0.5]);
    }
}
