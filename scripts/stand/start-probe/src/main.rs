//! Стенд: чёрный кадр на старте окна. Windows Graphics Capture снимает само окно процесса (не экран:
//! перекрытие другими окнами не мешает, чужого на снимке нет) с момента, как его можно захватить;
//! для каждого кадра — доля почти чёрных пикселей и средний цвет. Ввода не делает, только смотрит.
//!
//! `start-probe [--seconds 90] [--after 3] [--out frames.csv] [--dump dir] -- <exe> [args...]`

use std::time::{Duration, Instant};

use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, Interface};

/// Видимое окно верхнего уровня процесса с самой большой площадью.
fn window_of(pid: u32) -> Option<HWND> {
    struct Acc {
        pid: u32,
        best: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` — наш `&mut Acc` на время EnumWindows.
        let acc = unsafe { &mut *(l.0 as *mut Acc) };
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
        if p == acc.pid && unsafe { IsWindowVisible(h) }.as_bool() {
            let mut r = Default::default();
            if unsafe { GetWindowRect(h, &mut r) }.is_ok() {
                let area = i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
                if area > 10_000 && acc.best.is_none_or(|(_, a)| area > a) {
                    acc.best = Some((h, area));
                }
            }
        }
        BOOL(1)
    }
    let mut acc = Acc { pid, best: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&raw mut acc as isize));
    }
    acc.best.map(|(h, _)| h)
}

fn cloaked(h: HWND) -> bool {
    let mut c = 0u32;
    let _ = unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, (&raw mut c).cast(), size_of::<u32>() as u32) };
    c != 0
}

fn main() -> windows::core::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let split = argv.iter().position(|a| a == "--").expect("-- <exe> [args]");
    let (opts, cmd) = (&argv[..split], &argv[split + 1..]);
    let (mut seconds, mut after, mut out_csv, mut dump) = (90.0f64, 3.0f64, None, None::<String>);
    for w in opts.chunks(2) {
        match w[0].as_str() {
            "--seconds" => seconds = w[1].parse().unwrap_or(90.0),
            "--after" => after = w[1].parse().unwrap_or(3.0),
            "--out" => out_csv = Some(w[1].clone()),
            "--dump" => dump = Some(w[1].clone()),
            _ => {}
        }
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    let (dev, ctx) = d3d()?;
    let dxgi: IDXGIDevice = dev.cast()?;
    let winrt_dev: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;

    let t0 = Instant::now();
    let mut child = std::process::Command::new(&cmd[0]).args(&cmd[1..]).spawn().expect("запуск");
    let pid = child.id();
    let ms = || t0.elapsed().as_secs_f64() * 1000.0;
    let mut rows = vec!["t_ms,event,cloaked,black_pct,mean_r,mean_g,mean_b,w,h".to_string()];

    // Окно — как только видно; захват — как только Windows его разрешает.
    let deadline = Duration::from_secs_f64(seconds);
    let hwnd = loop {
        if let Some(h) = window_of(pid) {
            rows.push(format!("{:.1},window-visible,{},,,,,,", ms(), cloaked(h) as u8));
            break h;
        }
        if t0.elapsed() > deadline {
            let _ = child.kill();
            eprintln!("окно не появилось");
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let item: GraphicsCaptureItem = loop {
        match unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(hwnd) } {
            Ok(i) => break i,
            Err(_) if t0.elapsed() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => return Err(e),
        }
    };
    let mut size = item.Size()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt_dev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    session.StartCapture()?;
    rows.push(format!("{:.1},capture-started,{},,,,,,", ms(), cloaked(hwnd) as u8));

    let stop = t0.elapsed() + Duration::from_secs_f64(after);
    let (mut frames, mut black, mut dumped) = (0u32, 0u32, 0u32);
    while t0.elapsed() < stop {
        let Ok(frame) = pool.TryGetNextFrame() else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        let cs = frame.ContentSize()?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let (w, h) = ((cs.Width as u32).min(desc.Width), (cs.Height as u32).min(desc.Height));
        let staging = staging(&dev, desc.Width, desc.Height)?;
        unsafe { ctx.CopyResource(&staging, &tex) };
        let (pct, mean) = stats(&ctx, &staging, w, h, dump.as_ref().filter(|_| dumped < 6).map(|d| format!("{d}/f{frames:03}.bmp")))?;
        if dump.is_some() && dumped < 6 {
            dumped += 1;
        }
        frames += 1;
        if pct >= 50.0 {
            black += 1;
        }
        rows.push(format!("{:.1},frame,{},{pct:.1},{},{},{},{w},{h}", ms(), cloaked(hwnd) as u8, mean[0], mean[1], mean[2]));
        frame.Close()?;
        if cs.Width != size.Width || cs.Height != size.Height {
            size = cs;
            pool.Recreate(&winrt_dev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        }
    }
    session.Close()?;
    let _ = child.kill();
    if let Some(p) = out_csv {
        let _ = std::fs::write(p, rows.join("\n"));
    }
    println!("frames={frames} black={black}");
    Ok(())
}

fn d3d() -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut dev, mut ctx) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )?;
    }
    Ok((dev.expect("device"), ctx.expect("context")))
}

fn staging(dev: &ID3D11Device, w: u32, h: u32) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut t = None;
    unsafe { dev.CreateTexture2D(&desc, None, Some(&mut t))? };
    Ok(t.expect("texture"))
}

/// Доля почти чёрных проб (каждый канал < 12) и средний цвет (R, G, B) на сетке 4×4 px; по желанию — BMP.
fn stats(
    ctx: &ID3D11DeviceContext,
    t: &ID3D11Texture2D,
    w: u32,
    h: u32,
    bmp: Option<String>,
) -> windows::core::Result<(f64, [u32; 3])> {
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(t, 0, D3D11_MAP_READ, 0, Some(&mut m))? };
    let row = |y: u32| unsafe { std::slice::from_raw_parts(m.pData.cast::<u8>().add((y * m.RowPitch) as usize), (w * 4) as usize) };
    let (mut n, mut dark, mut sum) = (0u64, 0u64, [0u64; 3]);
    for y in (0..h).step_by(4) {
        let r = row(y);
        for x in (0..w as usize).step_by(4) {
            let (b, g, rr) = (r[x * 4], r[x * 4 + 1], r[x * 4 + 2]);
            n += 1;
            if rr < 12 && g < 12 && b < 12 {
                dark += 1;
            }
            sum[0] += u64::from(rr);
            sum[1] += u64::from(g);
            sum[2] += u64::from(b);
        }
    }
    if let Some(path) = bmp {
        let size = 54 + w * h * 4;
        let mut f = Vec::with_capacity(size as usize);
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&size.to_le_bytes());
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&54u32.to_le_bytes());
        f.extend_from_slice(&40u32.to_le_bytes());
        f.extend_from_slice(&(w as i32).to_le_bytes());
        f.extend_from_slice(&(-(h as i32)).to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&32u16.to_le_bytes());
        f.extend_from_slice(&[0; 24]);
        for y in 0..h {
            f.extend_from_slice(row(y));
        }
        let _ = std::fs::write(path, f);
    }
    unsafe { ctx.Unmap(t, 0) };
    let n1 = n.max(1);
    Ok((dark as f64 * 100.0 / n1 as f64, [(sum[0] / n1) as u32, (sum[1] / n1) as u32, (sum[2] / n1) as u32]))
}
