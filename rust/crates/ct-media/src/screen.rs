//! Enumeração de monitores e captura de tela nativa no Windows
//! (DXGI Desktop Duplication com fallback automático para GDI BitBlt).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use ct_core::protocol::QualityPreset;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSource {
    pub id: String,
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Índice do monitor na enumeração (usado para localizar o IDXGIOutput correspondente).
    pub monitor_index: usize,
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
    worker: Option<JoinHandle<()>>,
    frames_rx: watch::Receiver<Option<CapturedVideoFrame>>,
}

impl ScreenCapturer {
    pub fn start(source_id: String, preset: QualityPreset) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = watch::channel(None);
        let run_flag = running.clone();

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
                    capture_loop_windows(source, preset, run_flag, tx);
                }
                #[cfg(not(windows))]
                {
                    capture_loop_fallback(preset, run_flag, tx);
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

/// Calcula a dimensão final preservando a proporção dentro do preset escolhido.
fn fit_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (max_w.max(2), max_h.max(2));
    }
    if src_w <= max_w && src_h <= max_h {
        return (src_w, src_h);
    }
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let w = ((src_w as f64 * scale).round() as u32).max(2);
    let h = ((src_h as f64 * scale).round() as u32).max(2);
    (w, h)
}

/// Converte BGRA (Windows D3D11/GDI) para RGBA e faz downsample se necessário.
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
    preset: QualityPreset,
    running: Arc<AtomicBool>,
    tx: watch::Sender<Option<CapturedVideoFrame>>,
) {
    // Tenta DXGI Desktop Duplication (GPU, baixíssima CPU); se falhar ou perder acesso,
    // cai automaticamente para GDI BitBlt no retângulo exato do monitor.
    if let Err(err) = capture_loop_dxgi(&source, preset, &running, &tx) {
        tracing::warn!("DXGI Desktop Duplication indisponível ({err}); usando fallback GDI BitBlt");
        if running.load(Ordering::Relaxed) {
            capture_loop_gdi(&source, preset, &running, &tx);
        }
    }
}

#[cfg(windows)]
fn capture_loop_dxgi(
    source: &ScreenSource,
    preset: QualityPreset,
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
        CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIFactory1,
        IDXGIOutput1,
    };
    use windows::core::Interface;

    unsafe {
        let factory: IDXGIFactory1 =
            CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;

        // Localiza o IDXGIAdapter + IDXGIOutput cujo DesktopCoordinates bate com o monitor escolhido.
        let mut matched: Option<(windows::Win32::Graphics::Dxgi::IDXGIAdapter1, IDXGIOutput1)> =
            None;
        let mut fallback_first: Option<(
            windows::Win32::Graphics::Dxgi::IDXGIAdapter1,
            IDXGIOutput1,
        )> = None;

        let mut adapter_idx = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_idx) {
            let mut output_idx = 0u32;
            while let Ok(output) = adapter.EnumOutputs(output_idx) {
                if let (Ok(desc), Ok(output1)) = (output.GetDesc(), output.cast::<IDXGIOutput1>()) {
                    let r = desc.DesktopCoordinates;
                    if fallback_first.is_none() {
                        fallback_first = Some((adapter.clone(), output1.clone()));
                    }
                    if r.left == source.left && r.top == source.top {
                        matched = Some((adapter.clone(), output1));
                        break;
                    }
                }
                output_idx += 1;
            }
            if matched.is_some() {
                break;
            }
            adapter_idx += 1;
        }

        let (adapter, output1) = matched
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
        let frame_interval = Duration::from_secs_f64(1.0 / f64::from(preset.fps.max(1)));
        let mut seq = 0u64;

        while running.load(Ordering::Relaxed) {
            let started = Instant::now();
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            match duplication.AcquireNextFrame(50, &mut frame_info, &mut resource) {
                Ok(()) => {
                    if let Some(res) = resource
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
                                let total_bytes = pitch * (desc.Height as usize);
                                let slice = std::slice::from_raw_parts(
                                    mapped.pData as *const u8,
                                    total_bytes,
                                );
                                let (dst_w, dst_h) = fit_dimensions(
                                    desc.Width,
                                    desc.Height,
                                    preset.width,
                                    preset.height,
                                );
                                let rgba = convert_bgra_to_rgba_scaled(
                                    slice,
                                    pitch,
                                    desc.Width,
                                    desc.Height,
                                    dst_w,
                                    dst_h,
                                );
                                context.Unmap(st_tex, 0);

                                seq += 1;
                                let _ = tx.send(Some(CapturedVideoFrame {
                                    width: dst_w,
                                    height: dst_h,
                                    rgba: Arc::new(rgba),
                                    seq,
                                }));
                            }
                        }
                    }
                    let _ = duplication.ReleaseFrame();
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    // Tela estática: sem novo frame do compositor
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
    preset: QualityPreset,
    running: &Arc<AtomicBool>,
    tx: &watch::Sender<Option<CapturedVideoFrame>>,
) {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SelectObject,
        SetStretchBltMode, StretchBlt,
    };

    let (dst_w, dst_h) = fit_dimensions(source.width, source.height, preset.width, preset.height);
    let frame_interval = Duration::from_secs_f64(1.0 / f64::from(preset.fps.max(1)));
    let mut seq = 0u64;

    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return;
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: dst_w as i32,
            biHeight: -(dst_h as i32), // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(
            Some(mem_dc),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        ) else {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            return;
        };
        let old = SelectObject(mem_dc, dib.into());
        SetStretchBltMode(mem_dc, HALFTONE);

        let pitch = (dst_w as usize) * 4;
        let total = pitch * (dst_h as usize);

        while running.load(Ordering::Relaxed) {
            let started = Instant::now();
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
                seq += 1;
                let _ = tx.send(Some(CapturedVideoFrame {
                    width: dst_w,
                    height: dst_h,
                    rgba: Arc::new(rgba),
                    seq,
                }));
            }

            let elapsed = started.elapsed();
            if elapsed < frame_interval {
                thread::sleep(frame_interval - elapsed);
            }
        }

        SelectObject(mem_dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
    }
}

#[cfg(not(windows))]
fn capture_loop_fallback(
    preset: QualityPreset,
    running: Arc<AtomicBool>,
    tx: watch::Sender<Option<CapturedVideoFrame>>,
) {
    let frame_interval = Duration::from_secs_f64(1.0 / f64::from(preset.fps.max(1)));
    let (w, h) = fit_dimensions(1280, 720, preset.width.min(640), preset.height.min(360));
    let mut seq = 0u64;
    while running.load(Ordering::Relaxed) {
        seq += 1;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for chunk in buf.chunks_exact_mut(4) {
            chunk[0] = 28;
            chunk[1] = 32;
            chunk[2] = 42;
            chunk[3] = 255;
        }
        let _ = tx.send(Some(CapturedVideoFrame {
            width: w,
            height: h,
            rgba: Arc::new(buf),
            seq,
        }));
        thread::sleep(frame_interval);
    }
}
