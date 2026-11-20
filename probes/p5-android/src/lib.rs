//! P5 (Android half): does SDL3 + wgpu present a frame on Android, built from Rust with no
//! hand-written Java/Kotlin?
//!
//! Built as `libmain.so`. SDL's own Java activity (`org.libsdl.app.SDLActivity`, shipped from SDL's
//! source, not written by us) loads `libSDL3.so` and this library, then calls `SDL_main` on its own
//! thread. The probe clears the surface for a number of frames, renders the same clear offscreen and
//! reads one pixel back — the P1 check — and logs one `p5 key=value ...` line to logcat (tag
//! `leonis`). It also logs every lifecycle event SDL delivers, as input for P7.
//!
//! The probe reads its mode from the `debug.leonis.p5` system property (`adb shell setprop`)
//! rather than from intent extras, because reading extras needs Java. Values: empty (normal run) or
//! `plant-wrong-colour` (the negative control).

use std::ffi::{CString, c_char, c_int};
use std::time::Instant;

const FRAMES: u32 = 300;
const CLEAR: wgpu::Color = wgpu::Color { r: 0.0, g: 0.5, b: 1.0, a: 1.0 };

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
}

/// Writes one line to logcat with tag `leonis` at INFO priority.
fn log(line: &str) {
    let tag = c"leonis";
    let text = CString::new(line.replace('\0', "?")).unwrap_or_default();
    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    unsafe {
        __android_log_write(4, tag.as_ptr(), text.as_ptr());
    }
}

/// Reads an Android system property; empty if unset.
fn property(name: &str) -> String {
    let Ok(name) = CString::new(name) else { return String::new() };
    let mut value = [0 as c_char; 92]; // PROP_VALUE_MAX
    // SAFETY: `name` is NUL-terminated and `value` is the PROP_VALUE_MAX-sized buffer the function
    // requires; it writes at most that many bytes including the terminator.
    let len = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
    let bytes: Vec<u8> = value[..len.max(0) as usize].iter().map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Entry point called by SDL's Java activity (via SDL's `nativeRunMain`).
///
/// # Safety
/// Called by SDL with a valid argc/argv pair; neither is read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    let plant = property("debug.leonis.p5") == "plant-wrong-colour";
    match std::panic::catch_unwind(|| run(plant)) {
        Ok(Ok(true)) => {
            log("p5 exit=0");
            0
        }
        Ok(Ok(false)) => {
            log("p5 exit=1");
            1
        }
        Ok(Err(e)) => {
            log(&format!("p5 result=error error={e:?}"));
            log("p5 exit=2");
            2
        }
        Err(_) => {
            log("p5 result=panic");
            log("p5 exit=2");
            2
        }
    }
}

fn run(plant: bool) -> Result<bool, String> {
    let sdl = sdl3::init().map_err(|e| format!("sdl init: {e}"))?;
    let video = sdl.video().map_err(|e| format!("video init: {e}"))?;
    let driver = video.current_video_driver().to_string();
    // On Android the window is the activity's full-screen surface; the requested size is ignored.
    let window = video.window("leonis P5", 640, 400).build().map_err(|e| format!("window: {e}"))?;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    // SAFETY: `window` is declared before `surface` and never re-bound, so it is dropped after the
    // surface (see docs/probes/p1.md for the use-after-free a re-binding caused).
    let surface = unsafe {
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(&window, &window)
            .map_err(|e| format!("raw handle: {e}"))?;
        instance.create_surface_unsafe(target).map_err(|e| format!("create surface: {e}"))?
    };
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .map_err(|e| format!("adapter: {e}"))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| format!("device: {e}"))?;

    let (w, h) = window.size_in_pixels();
    let mut config = surface
        .get_default_config(&adapter, w.max(1), h.max(1))
        .ok_or("surface not supported by adapter")?;
    surface.configure(&device, &config);
    log(&format!(
        "p5 setup driver={driver} backend={:?} adapter={:?} device_type={:?} driver_name={:?} format={:?} present_mode={:?} size={}x{}",
        info.backend, info.name, info.device_type, info.driver, config.format, config.present_mode, config.width, config.height
    ));

    let mut events = sdl.event_pump().map_err(|e| format!("event pump: {e}"))?;
    let (mut presented, mut skipped, mut occluded, mut reconfigures) = (0u32, 0u32, 0u32, 0u32);
    let start = Instant::now();
    'frames: for _ in 0..FRAMES {
        for event in events.poll_iter() {
            use sdl3::event::{Event, WindowEvent};
            match event {
                Event::Quit { .. } => {
                    log("p5 event=quit");
                    break 'frames;
                }
                Event::Window { win_event: WindowEvent::PixelSizeChanged(nw, nh), .. } => {
                    config.width = nw.max(1) as u32;
                    config.height = nh.max(1) as u32;
                    surface.configure(&device, &config);
                    reconfigures += 1;
                    log(&format!("p5 event=pixel_size_changed size={nw}x{nh}"));
                }
                Event::AppWillEnterBackground { .. }
                | Event::AppDidEnterBackground { .. }
                | Event::AppWillEnterForeground { .. }
                | Event::AppDidEnterForeground { .. }
                | Event::AppLowMemory { .. }
                | Event::AppTerminating { .. } => log(&format!("p5 event={event:?}")),
                Event::Window { win_event, .. } => log(&format!("p5 event=window {win_event:?}")),
                _ => {}
            }
        }
        let texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&device, &config);
                reconfigures += 1;
                skipped += 1;
                continue;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                occluded += 1;
                std::thread::sleep(std::time::Duration::from_millis(16));
                continue;
            }
            other => return Err(format!("surface texture: {other:?}")),
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        clear(&device, &queue, &view, CLEAR);
        queue.present(texture);
        presented += 1;
        if presented == 1 {
            // run-on-device.sh waits for this line before taking its screenshot.
            log("p5 first_frame");
        }
    }
    let elapsed = start.elapsed();

    let readback_colour = if plant { wgpu::Color::RED } else { CLEAR };
    let pixel = readback(&device, &queue, config.format, readback_colour)?;
    let expected = expected_pixel(config.format);
    let pass = presented > 0 && presented + skipped + occluded == FRAMES && pixel_close(pixel, expected);
    log(&format!(
        "p5 result={} plant={plant} frames={FRAMES} presented={presented} skipped={skipped} occluded={occluded} \
         reconfigures={reconfigures} pixel={pixel:?} expected={expected:?} ms={}",
        if pass { "pass" } else { "fail" },
        elapsed.as_millis()
    ));
    Ok(pass)
}

fn clear(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, colour: wgpu::Color) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(colour), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    colour: wgpu::Color,
) -> Result<[u8; 4], String> {
    let width = 64u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("p5 readback"),
        size: wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("p5 readback buffer"),
        size: u64::from(width) * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    clear(device, queue, &texture.create_view(&wgpu::TextureViewDescriptor::default()), colour);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: None },
        },
        wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("poll: {e}"))?;
    rx.recv().map_err(|e| format!("map channel: {e}"))?.map_err(|e| format!("map: {e}"))?;
    let data = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?;
    Ok([data[0], data[1], data[2], data[3]])
}

fn expected_pixel(format: wgpu::TextureFormat) -> [u8; 4] {
    let half = if format.is_srgb() { 188 } else { 128 };
    match format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => [255, half, 0, 255],
        _ => [0, half, 255, 255],
    }
}

fn pixel_close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
}
