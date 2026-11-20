//! P1: does an SDL3 window give wgpu a usable surface?
//!
//! Opens an SDL3 window, creates a wgpu surface from its raw window handle, clears it to a known
//! colour for a number of frames (resizing the window part-way through to exercise reconfiguration),
//! then renders the same clear offscreen and reads one pixel back.
//!
//! Prints one `p1 key=value ...` report line. Exit codes: 0 pass, 1 a check failed, 2 could not run.

use std::process::ExitCode;
use std::time::Instant;

const CLEAR: wgpu::Color = wgpu::Color { r: 0.0, g: 0.5, b: 1.0, a: 1.0 };

struct Args {
    frames: u32,
    resize_at: u32,
    /// Negative control: clear the readback texture with the wrong colour; the probe must fail.
    plant_wrong_colour: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { frames: 180, resize_at: 60, plant_wrong_colour: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |name: &str| -> Result<u32, String> {
            it.next()
                .ok_or_else(|| format!("{name} needs a value"))?
                .parse()
                .map_err(|e| format!("{name}: {e}"))
        };
        match a.as_str() {
            "--frames" => args.frames = value("--frames")?,
            "--resize-at" => args.resize_at = value("--resize-at")?,
            "--plant-wrong-colour" => args.plant_wrong_colour = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("p1: {e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            println!("p1 result=error error={e:?}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let sdl = sdl3::init().map_err(|e| format!("sdl init: {e}"))?;
    let video = sdl.video().map_err(|e| format!("video init: {e}"))?;
    let driver = video.current_video_driver().to_string();
    let mut window = video
        .window("leonis P1", 640, 400)
        .resizable()
        .build()
        .map_err(|e| format!("window: {e}"))?;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    // SAFETY: `window` outlives `surface`: both live until the end of this function and the
    // surface is declared after the window, so it is dropped first. Never re-bind `window` below
    // this point: a re-binding is a new local declared after `surface`, dropped BEFORE it (that
    // was a use-after-free, a double free at exit on lavapipe; see docs/probes/p1.md).
    let surface = unsafe {
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(&window, &window)
            .map_err(|e| format!("raw handle: {e}"))?;
        instance
            .create_surface_unsafe(target)
            .map_err(|e| format!("create surface: {e}"))?
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
        .get_default_config(&adapter, w, h)
        .ok_or("surface not supported by adapter")?;
    surface.configure(&device, &config);

    let mut events = sdl.event_pump().map_err(|e| format!("event pump: {e}"))?;
    let mut presented = 0u32;
    let mut reconfigures = 0u32;
    let mut skipped = 0u32;
    let start = Instant::now();
    'frames: for frame in 0..args.frames {
        if frame == args.resize_at {
            window.set_size(800, 500).map_err(|e| format!("set_size: {e}"))?;
        }
        for event in events.poll_iter() {
            match event {
                sdl3::event::Event::Quit { .. } => break 'frames,
                sdl3::event::Event::Window {
                    win_event: sdl3::event::WindowEvent::PixelSizeChanged(nw, nh),
                    ..
                } => {
                    config.width = nw.max(1) as u32;
                    config.height = nh.max(1) as u32;
                    surface.configure(&device, &config);
                    reconfigures += 1;
                }
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
            other => return Err(format!("surface texture: {other:?}")),
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        clear(&device, &queue, &view, CLEAR);
        queue.present(texture);
        presented += 1;
    }
    let elapsed = start.elapsed();

    let readback_colour = if args.plant_wrong_colour { wgpu::Color::RED } else { CLEAR };
    let pixel = readback(&device, &queue, config.format, readback_colour)?;
    let expected = expected_pixel(config.format);
    let pass = presented + skipped == args.frames
        && presented > 0
        && pixel_close(pixel, expected)
        && (args.resize_at >= args.frames || reconfigures > 0);
    println!(
        "p1 result={} driver={driver} backend={:?} adapter={:?} device_type={:?} format={:?} present_mode={:?} \
         size={}x{} frames={} presented={presented} skipped={skipped} reconfigures={reconfigures} \
         pixel={pixel:?} expected={expected:?} ms={}",
        if pass { "pass" } else { "fail" },
        info.backend,
        info.name,
        info.device_type,
        config.format,
        config.present_mode,
        config.width,
        config.height,
        args.frames,
        elapsed.as_millis(),
    );
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

/// Renders the clear into a 64x1 offscreen texture of `format` and returns the first pixel's bytes
/// in the texture's own byte order.
fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    colour: wgpu::Color,
) -> Result<[u8; 4], String> {
    let width = 64u32; // 64 px * 4 B = 256 B, the required row alignment.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("p1 readback"),
        size: wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("p1 readback buffer"),
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
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("poll: {e}"))?;
    rx.recv()
        .map_err(|e| format!("map channel: {e}"))?
        .map_err(|e| format!("map: {e}"))?;
    let data = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?;
    Ok([data[0], data[1], data[2], data[3]])
}

/// The clear colour's bytes in `format`'s byte order (sRGB formats encode 0.5 as 188).
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
