//! Enumeração de monitores e GPUs disponíveis, captura de tela nativa no Windows
//! (DXGI Desktop Duplication com escolha de GPU + fallback GDI BitBlt), alteração ao vivo
//! de resolução/FPS, detecção de região alterada (dirty-rect) e codificação JPEG 4:2:0.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use ct_core::protocol::{ResolutionId, VideoPacketHeader};
use tokio::sync::watch;

/// Sequência global monotônica de frames (nunca volta para 1 ao reiniciar a transmissão!).
static NEXT_FRAME_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_seq() -> u64 {
    NEXT_FRAME_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSource {
    pub id: String,
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub monitor_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapterInfo {
    /// `None` = automática (GPU conectada ao monitor escolhido).
    pub index: Option<u32>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CapturedVideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    pub seq: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct LiveCaptureConfig {
    pub max_width: u32,
    pub max_height: u32,
    pub fps: u32,
}

impl LiveCaptureConfig {
    pub fn from_res_fps(res: ResolutionId, fps: u32) -> Self {
        let (max_width, max_height) = res.dimensions();
        Self {
            max_width,
            max_height,
            fps: fps.clamp(5, 60),
        }
    }
}

/// Lista as GPUs DXGI disponíveis no PC (útil em notebooks híbridos ou PCs com 2+ GPUs).
pub fn list_gpu_adapters() -> Vec<GpuAdapterInfo> {
    #[allow(unused_mut)]
    let mut out = vec![GpuAdapterInfo {
        index: None,
        name: "Automática (GPU do monitor)".to_string(),
    }];

    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
        };
        unsafe {
            if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
                let mut idx = 0u32;
                while let Ok(adapter) = factory.EnumAdapters1(idx) {
                    if let Ok(desc) = adapter.GetDesc1() {
                        let is_sw = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
                        if !is_sw {
                            let end = desc
                                .Description
                                .iter()
                                .position(|&c| c == 0)
                                .unwrap_or(desc.Description.len());
                            let name = String::from_utf16_lossy(&desc.Description[..end])
                                .trim()
                                .to_string();
                            if !name.is_empty() {
                                out.push(GpuAdapterInfo {
                                    index: Some(idx),
                                    name: format!("GPU {idx}: {name}"),
                                });
                            }
                        }
                    }
                    idx += 1;
                }
            }
        }
    }

    out
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
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
            monitor_index: 0,
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
                name: format!("Tela {} ({}x{})", idx + 1, width, height),
                left: r.left,
                top: r.top,
                width,
                height,
                monitor_index: idx,
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
            name: "Tela 1 (1920x1080)".to_string(),
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
            monitor_index: 0,
        });
    }
    list
}

pub struct ScreenCapturer {
    running: Arc<AtomicBool>,
    config: Arc<RwLock<LiveCaptureConfig>>,
    worker: Option<JoinHandle<()>>,
    frames_rx: watch::Receiver<Option<CapturedVideoFrame>>,
}

impl ScreenCapturer {
    pub fn start(
        source_id: String,
        initial_cfg: LiveCaptureConfig,
        gpu_adapter_index: Option<u32>,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let config = Arc::new(RwLock::new(initial_cfg));
        let (tx, rx) = watch::channel(None);
        let run_flag = running.clone();
        let cfg_ref = config.clone();

        let source = list_screen_sources()
            .into_iter()
            .find(|s| s.id == source_id)
            .unwrap_or_else(|| {
                list_screen_sources()
                    .into_iter()
                    .next()
                    .unwrap_or(ScreenSource {
                        id: "screen:0".to_string(),
                        name: "Tela 1".to_string(),
                        left: 0,
                        top: 0,
                        width: 1920,
                        height: 1080,
                        monitor_index: 0,
                    })
            });

        let worker = thread::Builder::new()
            .name(format!("ct-screen-{}", source.id))
            .spawn(move || {
                #[cfg(windows)]
                {
                    capture_loop_windows(source, cfg_ref, gpu_adapter_index, run_flag, tx);
                }
                #[cfg(not(windows))]
                {
                    let _ = gpu_adapter_index;
                    capture_loop_fallback(cfg_ref, run_flag, tx);
                }
            })
            .ok();

        Self {
            running,
            config,
            worker,
            frames_rx: rx,
        }
    }

    pub fn update_config(&self, cfg: LiveCaptureConfig) {
        if let Ok(mut guard) = self.config.write() {
            *guard = cfg;
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

fn fit_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (max_w.max(2) & !1, max_h.max(2) & !1);
    }
    if src_w <= max_w && src_h <= max_h {
        return ((src_w & !1).max(2), (src_h & !1).max(2));
    }
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let w = (((src_w as f64 * scale).round() as u32) & !1).max(2);
    let h = (((src_h as f64 * scale).round() as u32) & !1).max(2);
    (w, h)
}

#[cfg(windows)]
fn convert_bgra_to_rgba_scaled(
    src: &[u8],
    src_pitch: usize,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    if src_w == dst_w && src_h == dst_h {
        let row_bytes = (dst_w as usize) * 4;
        for y in 0..dst_h as usize {
            let src_row = &src[y * src_pitch..y * src_pitch + row_bytes];
            let dst_row = &mut out[y * row_bytes..(y + 1) * row_bytes];
            for (s, d) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
        }
    } else {
        let sw = src_w as usize;
        let sh = src_h as usize;
        let dw = dst_w as usize;
        let dh = dst_h as usize;
        for y in 0..dh {
            let sy = (y * sh) / dh;
            let src_row = &src[sy * src_pitch..];
            let dst_row = &mut out[y * dw * 4..(y + 1) * dw * 4];
            for x in 0..dw {
                let sx = (x * sw) / dw;
                let si = sx * 4;
                let di = x * 4;
                dst_row[di] = src_row[si + 2];
                dst_row[di + 1] = src_row[si + 1];
                dst_row[di + 2] = src_row[si];
                dst_row[di + 3] = 255;
            }
        }
    }
    out
}

#[cfg(windows)]
fn capture_loop_windows(
    source: ScreenSource,
    config: Arc<RwLock<LiveCaptureConfig>>,
    gpu_adapter_index: Option<u32>,
    running: Arc<AtomicBool>,
    tx: watch::Sender<Option<CapturedVideoFrame>>,
) {
    if let Err(err) = capture_loop_dxgi(&source, &config, gpu_adapter_index, &running, &tx) {
        tracing::warn!("DXGI Desktop Duplication indisponível ({err}); usando fallback GDI BitBlt");
        if running.load(Ordering::Relaxed) {
            capture_loop_gdi(&source, &config, &running, &tx);
        }
    }
}

#[cfg(windows)]
fn capture_loop_dxgi(
    source: &ScreenSource,
    config: &Arc<RwLock<LiveCaptureConfig>>,
    preferred_gpu: Option<u32>,
    running: &Arc<AtomicBool>,
    tx: &watch::Sender<Option<CapturedVideoFrame>>,
) -> Result<(), String> {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
        IDXGIFactory1, IDXGIOutput1,
    };
    use windows::core::Interface;

    unsafe {
        let factory: IDXGIFactory1 =
            CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;

        let mut preferred_match: Option<(IDXGIAdapter1, IDXGIOutput1)> = None;
        let mut exact_match: Option<(IDXGIAdapter1, IDXGIOutput1)> = None;
        let mut preferred_any: Option<(IDXGIAdapter1, IDXGIOutput1)> = None;
        let mut fallback_first: Option<(IDXGIAdapter1, IDXGIOutput1)> = None;

        let mut adapter_idx = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_idx) {
            let is_pref = preferred_gpu == Some(adapter_idx);
            let mut output_idx = 0u32;
            while let Ok(output) = adapter.EnumOutputs(output_idx) {
                if let (Ok(desc), Ok(output1)) = (output.GetDesc(), output.cast::<IDXGIOutput1>()) {
                    let r = desc.DesktopCoordinates;
                    let coords_match = r.left == source.left && r.top == source.top;
                    if fallback_first.is_none() {
                        fallback_first = Some((adapter.clone(), output1.clone()));
                    }
                    if is_pref && preferred_any.is_none() {
                        preferred_any = Some((adapter.clone(), output1.clone()));
                    }
                    if coords_match {
                        if is_pref {
                            preferred_match = Some((adapter.clone(), output1.clone()));
                        }
                        if exact_match.is_none() {
                            exact_match = Some((adapter.clone(), output1));
                        }
                    }
                }
                output_idx += 1;
            }
            adapter_idx += 1;
        }

        // Prioridade: GPU escolhida com o monitor exato -> monitor exato na GPU dona -> GPU escolhida -> primeiro monitor
        let (adapter, output1) = preferred_match
            .or(exact_match)
            .or(preferred_any)
            .or(fallback_first)
            .ok_or_else(|| "Nenhum monitor DXGI encontrado".to_string())?;

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(|e| format!("D3D11CreateDevice: {e}"))?;

        let device = device.ok_or_else(|| "ID3D11Device nulo".to_string())?;
        let context = context.ok_or_else(|| "ID3D11DeviceContext nulo".to_string())?;

        let duplication = output1
            .DuplicateOutput(&device)
            .map_err(|e| format!("DuplicateOutput: {e}"))?;

        let mut staging: Option<ID3D11Texture2D> = None;
        let mut staging_size = (0u32, 0u32);
        let mut local_count = 0u64;
        let mut last_sent_cfg = (0u32, 0u32);

        while running.load(Ordering::Relaxed) {
            let cfg = *config.read().unwrap();
            let frame_interval = Duration::from_secs_f64(1.0 / f64::from(cfg.fps.max(1)));
            let started = Instant::now();
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            let cfg_changed = last_sent_cfg != (cfg.max_width, cfg.max_height);

            match duplication.AcquireNextFrame(100, &mut frame_info, &mut resource) {
                Ok(()) => {
                    let has_new_image = local_count == 0
                        || cfg_changed
                        || frame_info.LastPresentTime != 0
                        || frame_info.AccumulatedFrames > 0;
                    if has_new_image
                        && let Some(res) = resource
                        && let Ok(tex) = res.cast::<ID3D11Texture2D>()
                    {
                        let mut desc = D3D11_TEXTURE2D_DESC::default();
                        tex.GetDesc(&mut desc);

                        if staging.is_none() || staging_size != (desc.Width, desc.Height) {
                            let mut st_desc = desc;
                            st_desc.Usage = D3D11_USAGE_STAGING;
                            st_desc.BindFlags = 0;
                            st_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                            st_desc.MiscFlags = 0;
                            let mut st_tex = None;
                            if device
                                .CreateTexture2D(&st_desc, None, Some(&mut st_tex))
                                .is_ok()
                            {
                                staging = st_tex;
                                staging_size = (desc.Width, desc.Height);
                            }
                        }

                        if let Some(ref st_tex) = staging {
                            context.CopyResource(st_tex, &tex);
                            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                            if context
                                .Map(st_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                                .is_ok()
                            {
                                let pitch = mapped.RowPitch as usize;
                                let row_bytes = (desc.Width as usize) * 4;
                                if !mapped.pData.is_null() && pitch >= row_bytes && desc.Height > 0
                                {
                                    let total_bytes = pitch * (desc.Height as usize);
                                    let slice = std::slice::from_raw_parts(
                                        mapped.pData as *const u8,
                                        total_bytes,
                                    );
                                    let (dst_w, dst_h) = fit_dimensions(
                                        desc.Width,
                                        desc.Height,
                                        cfg.max_width,
                                        cfg.max_height,
                                    );
                                    let rgba = convert_bgra_to_rgba_scaled(
                                        slice,
                                        pitch,
                                        desc.Width,
                                        desc.Height,
                                        dst_w,
                                        dst_h,
                                    );
                                    local_count += 1;
                                    last_sent_cfg = (cfg.max_width, cfg.max_height);
                                    let _ = tx.send(Some(CapturedVideoFrame {
                                        width: dst_w,
                                        height: dst_h,
                                        rgba: Arc::new(rgba),
                                        seq: next_seq(),
                                    }));
                                }
                                context.Unmap(st_tex, 0);
                            }
                        }
                    }
                    let _ = duplication.ReleaseFrame();
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    // Se o usuário mudou a resolução ao vivo com a tela parada, re-emite o staging atual!
                    if cfg_changed && let Some(ref st_tex) = staging {
                        let (sw, sh) = staging_size;
                        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                        if context
                            .Map(st_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                            .is_ok()
                        {
                            let pitch = mapped.RowPitch as usize;
                            let row_bytes = (sw as usize) * 4;
                            if !mapped.pData.is_null() && pitch >= row_bytes && sh > 0 {
                                let total_bytes = pitch * (sh as usize);
                                let slice = std::slice::from_raw_parts(
                                    mapped.pData as *const u8,
                                    total_bytes,
                                );
                                let (dst_w, dst_h) =
                                    fit_dimensions(sw, sh, cfg.max_width, cfg.max_height);
                                let rgba = convert_bgra_to_rgba_scaled(
                                    slice, pitch, sw, sh, dst_w, dst_h,
                                );
                                last_sent_cfg = (cfg.max_width, cfg.max_height);
                                let _ = tx.send(Some(CapturedVideoFrame {
                                    width: dst_w,
                                    height: dst_h,
                                    rgba: Arc::new(rgba),
                                    seq: next_seq(),
                                }));
                            }
                            context.Unmap(st_tex, 0);
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("AcquireNextFrame: {e}"));
                }
            }

            let elapsed = started.elapsed();
            if elapsed < frame_interval {
                thread::sleep(frame_interval - elapsed);
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn capture_loop_gdi(
    source: &ScreenSource,
    config: &Arc<RwLock<LiveCaptureConfig>>,
    running: &Arc<AtomicBool>,
    tx: &watch::Sender<Option<CapturedVideoFrame>>,
) {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SelectObject,
        SetStretchBltMode, StretchBlt,
    };

    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return;
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        SetStretchBltMode(mem_dc, HALFTONE);

        let mut cur_size = (0u32, 0u32);
        let mut dib_opt = None;
        let mut old_obj = None;
        let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();

        while running.load(Ordering::Relaxed) {
            let cfg = *config.read().unwrap();
            let frame_interval = Duration::from_secs_f64(1.0 / f64::from(cfg.fps.max(1)));
            let started = Instant::now();
            let (dst_w, dst_h) =
                fit_dimensions(source.width, source.height, cfg.max_width, cfg.max_height);

            if cur_size != (dst_w, dst_h) {
                if let Some(old) = old_obj.take() {
                    SelectObject(mem_dc, old);
                }
                if let Some(old_dib) = dib_opt.take() {
                    let _ = DeleteObject(old_dib);
                }
                let mut bmi = BITMAPINFO::default();
                bmi.bmiHeader = BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: dst_w as i32,
                    biHeight: -(dst_h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                };
                bits_ptr = std::ptr::null_mut();
                if let Ok(dib) = CreateDIBSection(
                    Some(mem_dc),
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bits_ptr,
                    None,
                    0,
                ) {
                    old_obj = Some(SelectObject(mem_dc, dib.into()));
                    dib_opt = Some(dib.into());
                    cur_size = (dst_w, dst_h);
                } else {
                    break;
                }
            }

            let pitch = (dst_w as usize) * 4;
            let total = pitch * (dst_h as usize);
            let ok = if dst_w == source.width && dst_h == source.height {
                BitBlt(
                    mem_dc,
                    0,
                    0,
                    dst_w as i32,
                    dst_h as i32,
                    Some(screen_dc),
                    source.left,
                    source.top,
                    SRCCOPY,
                )
                .is_ok()
            } else {
                StretchBlt(
                    mem_dc,
                    0,
                    0,
                    dst_w as i32,
                    dst_h as i32,
                    Some(screen_dc),
                    source.left,
                    source.top,
                    source.width as i32,
                    source.height as i32,
                    SRCCOPY,
                )
                .as_bool()
            };

            if ok && !bits_ptr.is_null() {
                let slice = std::slice::from_raw_parts(bits_ptr as *const u8, total);
                let rgba = convert_bgra_to_rgba_scaled(slice, pitch, dst_w, dst_h, dst_w, dst_h);
                let _ = tx.send(Some(CapturedVideoFrame {
                    width: dst_w,
                    height: dst_h,
                    rgba: Arc::new(rgba),
                    seq: next_seq(),
                }));
            }

            let elapsed = started.elapsed();
            if elapsed < frame_interval {
                thread::sleep(frame_interval - elapsed);
            }
        }

        if let Some(old) = old_obj {
            SelectObject(mem_dc, old);
        }
        if let Some(dib) = dib_opt {
            let _ = DeleteObject(dib);
        }
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
    }
}

#[cfg(not(windows))]
fn capture_loop_fallback(
    config: Arc<RwLock<LiveCaptureConfig>>,
    running: Arc<AtomicBool>,
    tx: watch::Sender<Option<CapturedVideoFrame>>,
) {
    while running.load(Ordering::Relaxed) {
        let cfg = *config.read().unwrap();
        let frame_interval = Duration::from_secs_f64(1.0 / f64::from(cfg.fps.max(1)));
        let (w, h) = fit_dimensions(1280, 720, cfg.max_width.min(640), cfg.max_height.min(360));
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for chunk in buf.chunks_exact_mut(4) {
            chunk[0] = 36;
            chunk[1] = 42;
            chunk[2] = 56;
            chunk[3] = 255;
        }
        let _ = tx.send(Some(CapturedVideoFrame {
            width: w,
            height: h,
            rgba: Arc::new(buf),
            seq: next_seq(),
        }));
        thread::sleep(frame_interval);
    }
}

// ============================================================================
// Codificador Delta (Dirty-Rect) + JPEG 4:2:0 e Canvas de Decodificação
// ============================================================================

pub struct DeltaVideoEncoder {
    prev_w: u32,
    prev_h: u32,
    prev_rgba: Vec<u8>,
    frames_since_key: u32,
}

impl Default for DeltaVideoEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaVideoEncoder {
    pub fn new() -> Self {
        Self {
            prev_w: 0,
            prev_h: 0,
            prev_rgba: Vec::new(),
            frames_since_key: 0,
        }
    }

    pub fn encode(
        &mut self,
        frame: &CapturedVideoFrame,
        quality: u8,
        force_keyframe: bool,
    ) -> Option<(VideoPacketHeader, Vec<u8>)> {
        let w = frame.width;
        let h = frame.height;
        if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
            return None;
        }

        let size_changed =
            self.prev_w != w || self.prev_h != h || self.prev_rgba.len() != frame.rgba.len();
        let need_key = force_keyframe || size_changed || self.frames_since_key >= 60;

        let (rx, ry, rw, rh, is_key) = if need_key {
            (0, 0, w, h, true)
        } else {
            match find_dirty_rect(&self.prev_rgba, &frame.rgba, w, h) {
                None => return None,
                Some((x, y, rw, rh)) => {
                    if u64::from(rw) * u64::from(rh) * 100 >= u64::from(w) * u64::from(h) * 65 {
                        (0, 0, w, h, true)
                    } else {
                        (x, y, rw, rh, false)
                    }
                }
            }
        };

        let jpeg = encode_region_jpeg(&frame.rgba, w, rx, ry, rw, rh, quality)?;

        self.prev_w = w;
        self.prev_h = h;
        self.prev_rgba.clear();
        self.prev_rgba.extend_from_slice(&frame.rgba);
        if is_key {
            self.frames_since_key = 0;
        } else {
            self.frames_since_key += 1;
        }

        let hdr = VideoPacketHeader {
            full_width: w,
            full_height: h,
            x: rx,
            y: ry,
            width: rw,
            height: rh,
            seq: frame.seq,
            is_keyframe: is_key,
        };
        Some((hdr, jpeg))
    }
}

fn find_dirty_rect(prev: &[u8], cur: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let w_usize = w as usize;
    let h_usize = h as usize;
    let mut min_x = w_usize;
    let mut min_y = h_usize;
    let mut max_x = 0usize;
    let mut max_y = 0usize;

    let row_bytes = w_usize * 4;
    for y in 0..h_usize {
        let row_off = y * row_bytes;
        let prev_row = &prev[row_off..row_off + row_bytes];
        let cur_row = &cur[row_off..row_off + row_bytes];
        if prev_row == cur_row {
            continue;
        }
        if y < min_y {
            min_y = y;
        }
        max_y = y;
        for x in 0..min_x {
            let p = x * 4;
            if prev_row[p..p + 4] != cur_row[p..p + 4] {
                min_x = x;
                break;
            }
        }
        for x in (max_x..w_usize).rev() {
            let p = x * 4;
            if prev_row[p..p + 4] != cur_row[p..p + 4] {
                max_x = x;
                break;
            }
        }
    }

    if min_x > max_x || min_y > max_y {
        return None;
    }

    let x0 = (min_x & !15) as u32;
    let y0 = (min_y & !15) as u32;
    let x1 = ((max_x + 16) & !15).min(w_usize) as u32;
    let y1 = ((max_y + 16) & !15).min(h_usize) as u32;
    let rw = (x1 - x0).max(2).min(w.saturating_sub(x0));
    let rh = (y1 - y0).max(2).min(h.saturating_sub(y0));
    if rw == 0 || rh == 0 {
        return None;
    }
    Some((x0, y0, rw, rh))
}

fn encode_region_jpeg(
    full_rgba: &[u8],
    full_w: u32,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    quality: u8,
) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(48 * 1024);
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality);
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);

    if rx == 0 && ry == 0 && rw == full_w {
        let slice_len = (rw as usize) * (rh as usize) * 4;
        if full_rgba.len() < slice_len {
            return None;
        }
        encoder
            .encode(
                &full_rgba[..slice_len],
                rw as u16,
                rh as u16,
                jpeg_encoder::ColorType::Rgba,
            )
            .ok()?;
    } else {
        let mut sub = Vec::with_capacity((rw as usize) * (rh as usize) * 4);
        let full_stride = (full_w as usize) * 4;
        let rx_bytes = (rx as usize) * 4;
        let rw_bytes = (rw as usize) * 4;
        for row in ry..(ry + rh) {
            let start = (row as usize) * full_stride + rx_bytes;
            sub.extend_from_slice(&full_rgba[start..start + rw_bytes]);
        }
        encoder
            .encode(&sub, rw as u16, rh as u16, jpeg_encoder::ColorType::Rgba)
            .ok()?;
    }
    Some(out)
}

#[derive(Debug)]
pub enum CanvasUpdate {
    Frame(CapturedVideoFrame),
    NeedKeyframe,
    Ignored,
}

/// Canvas persistente no receptor por stream remoto:
/// - Keyframes SEMPRE redefinem o canvas (evita travamento ao reiniciar ou mudar resolução).
/// - Deltas com dimensão diferente ou antes de keyframe pedem novo keyframe automaticamente.
pub struct RemoteVideoCanvas {
    width: u32,
    height: u32,
    last_seq: u64,
    has_keyframe: bool,
    rgba: Vec<u8>,
}

impl Default for RemoteVideoCanvas {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteVideoCanvas {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            last_seq: 0,
            has_keyframe: false,
            rgba: Vec::new(),
        }
    }

    pub fn apply_packet(&mut self, hdr: VideoPacketHeader, jpeg: &[u8]) -> CanvasUpdate {
        if !hdr.is_keyframe {
            if !self.has_keyframe || hdr.full_width != self.width || hdr.full_height != self.height
            {
                return CanvasUpdate::NeedKeyframe;
            }
            if hdr.seq <= self.last_seq {
                return CanvasUpdate::Ignored;
            }
        }

        let Ok(img) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
            return CanvasUpdate::NeedKeyframe;
        };
        let patch = img.to_rgba8();
        let (pw, ph) = patch.dimensions();
        let patch_raw = patch.into_raw();

        if hdr.is_keyframe {
            self.width = pw;
            self.height = ph;
            self.rgba = patch_raw;
            self.has_keyframe = true;
        } else {
            let full_stride = (self.width as usize) * 4;
            let patch_stride = (pw as usize) * 4;
            let copy_w = pw.min(self.width.saturating_sub(hdr.x)) as usize;
            let copy_h = ph.min(self.height.saturating_sub(hdr.y)) as usize;
            let copy_bytes = copy_w * 4;
            for row in 0..copy_h {
                let dst_off = ((hdr.y as usize) + row) * full_stride + (hdr.x as usize) * 4;
                let src_off = row * patch_stride;
                if dst_off + copy_bytes <= self.rgba.len()
                    && src_off + copy_bytes <= patch_raw.len()
                {
                    self.rgba[dst_off..dst_off + copy_bytes]
                        .copy_from_slice(&patch_raw[src_off..src_off + copy_bytes]);
                }
            }
        }

        self.last_seq = hdr.seq;
        CanvasUpdate::Frame(CapturedVideoFrame {
            width: self.width,
            height: self.height,
            rgba: Arc::new(self.rgba.clone()),
            seq: hdr.seq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_encoder_and_canvas_roundtrip_and_restart() {
        let mut enc = DeltaVideoEncoder::new();
        let mut canvas = RemoteVideoCanvas::new();

        let frame1 = CapturedVideoFrame {
            width: 64,
            height: 64,
            rgba: Arc::new(vec![40u8; 64 * 64 * 4]),
            seq: 100,
        };
        let (h1, j1) = enc.encode(&frame1, 70, true).unwrap();
        assert!(h1.is_keyframe);
        assert!(matches!(canvas.apply_packet(h1, &j1), CanvasUpdate::Frame(f) if f.seq == 100));

        // Novo stream com keyframe mesmo se seq menor reinicia o canvas sem travar!
        let frame_restart = CapturedVideoFrame {
            width: 64,
            height: 64,
            rgba: Arc::new(vec![90u8; 64 * 64 * 4]),
            seq: 1,
        };
        let (hr, jr) = enc.encode(&frame_restart, 70, true).unwrap();
        assert!(matches!(canvas.apply_packet(hr, &jr), CanvasUpdate::Frame(f) if f.seq == 1));

        // Delta com dimensão diferente solicita keyframe automaticamente
        let bad_delta = VideoPacketHeader {
            full_width: 128,
            full_height: 72,
            x: 0,
            y: 0,
            width: 16,
            height: 16,
            seq: 2,
            is_keyframe: false,
        };
        assert!(matches!(
            canvas.apply_packet(bad_delta, &jr),
            CanvasUpdate::NeedKeyframe
        ));
    }
}
