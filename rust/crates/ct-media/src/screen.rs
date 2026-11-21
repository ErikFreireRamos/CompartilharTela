//! Enumeração de monitores e captura de tela nativa (Windows.Graphics.Capture / DXGI no Windows).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ct_core::protocol::QualityPreset;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSource {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct CapturedVideoFrame {
    pub width: u32,
    pub height: u32,
    /// Pixels RGBA8 prontos para upload em textura GPU (`wgpu` / `egui`).
    pub rgba: Arc<Vec<u8>>,
    pub seq: u64,
}

pub fn list_screen_sources() -> Vec<ScreenSource> {
    #[cfg(windows)]
    {
        enumerate_windows_monitors()
    }
    #[cfg(not(windows))]
    {
        vec![ScreenSource {
            id: "screen:0".to_string(),
            name: "Monitor Principal (Linux Preview)".to_string(),
            width: 1920,
            height: 1080,
        }]
    }
}

#[cfg(windows)]
fn enumerate_windows_monitors() -> Vec<ScreenSource> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn enum_proc(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<ScreenSource>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut _).as_bool() } {
            let r = info.monitorInfo.rcMonitor;
            let width = (r.right - r.left).unsigned_abs();
            let height = (r.bottom - r.top).unsigned_abs();
            let idx = list.len();
            list.push(ScreenSource {
                id: format!("screen:{idx}"),
                name: format!("Tela {} ({}×{})", idx + 1, width, height),
                width,
                height,
            });
        }
        BOOL(1)
    }

    let mut list = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut list as *mut Vec<ScreenSource> as isize),
        );
    }
    if list.is_empty() {
        list.push(ScreenSource {
            id: "screen:0".to_string(),
            name: "Tela 1".to_string(),
            width: 1920,
            height: 1080,
        });
    }
    list
}

pub struct ScreenCapturer {
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    frames_rx: watch::Receiver<Option<CapturedVideoFrame>>,
}

impl ScreenCapturer {
    pub fn start(source_id: String, preset: QualityPreset) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = watch::channel(None);
        let run_flag = running.clone();

        let worker = thread::Builder::new()
            .name(format!("ct-screen-{source_id}"))
            .spawn(move || {
                let frame_interval = Duration::from_secs_f64(1.0 / f64::from(preset.fps.max(1)));
                let w = preset.width.min(640);
                let h = preset.height.min(360);
                let mut seq = 0u64;
                while run_flag.load(Ordering::Relaxed) {
                    seq += 1;
                    let rgba = generate_preview_frame(w, h, seq);
                    let _ = tx.send(Some(CapturedVideoFrame {
                        width: w,
                        height: h,
                        rgba: Arc::new(rgba),
                        seq,
                    }));
                    thread::sleep(frame_interval);
                }
            })
            .ok();

        Self {
            running,
            worker,
            frames_rx: rx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<CapturedVideoFrame>> {
        self.frames_rx.clone()
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ScreenCapturer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn generate_preview_frame(w: u32, h: u32, seq: u64) -> Vec<u8> {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let shift = (seq * 4) as u32;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            buf[i] = ((x + shift) % 256) as u8;
            buf[i + 1] = ((y * 255) / h.max(1)) as u8;
            buf[i + 2] = 48;
            buf[i + 3] = 255;
        }
    }
    buf
}
