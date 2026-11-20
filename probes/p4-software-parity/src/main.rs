//! P4: is software-adapter rendering identical across platforms?
//!
//! Renders one fixed scene offscreen (no window) and writes it as a PNG plus the SHA-256 of the raw
//! RGBA bytes. The scene exercises the places renderers usually disagree: triangle edge coverage
//! under 4x MSAA, colour interpolation, alpha blending of overlaps, bilinear texture filtering at a
//! non-integer offset, and sRGB encoding.
//!
//! `--adapter software` (default) demands the platform's software adapter (llvmpipe/lavapipe on
//! Linux, WARP on Windows) and exits 2 if there is none. `--adapter any` takes the default adapter,
//! for measuring a real GPU against the software baseline.
//!
//! Prints one `p4 key=value ...` line. Exit codes: 0 rendered, 2 could not run.

use std::process::ExitCode;

use sha2::{Digest, Sha256};

const SIZE: u32 = 256;
const SAMPLES: u32 = 4;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const SHADER: &str = r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) textured: f32,
};

@vertex
fn vs(@location(0) position: vec2<f32>, @location(1) colour: vec4<f32>,
      @location(2) uv: vec2<f32>, @location(3) textured: f32) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.colour = colour;
    out.uv = uv;
    out.textured = textured;
    return out;
}

@group(0) @binding(0) var checker: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(checker, linear_sampler, in.uv);
    return select(in.colour, in.colour * texel, in.textured > 0.5);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 2],
    colour: [f32; 4],
    uv: [f32; 2],
    textured: f32,
}

impl Vertex {
    fn flat(x: f32, y: f32, colour: [f32; 4]) -> Self {
        Self { position: [x, y], colour, uv: [0.0, 0.0], textured: 0.0 }
    }
}

/// Pixel coordinates (origin top left) to clip space.
fn clip(x: f32, y: f32) -> (f32, f32) {
    let s = SIZE as f32;
    (x / s * 2.0 - 1.0, 1.0 - y / s * 2.0)
}

fn quad(out: &mut Vec<Vertex>, corners: [(f32, f32); 4], colours: [[f32; 4]; 4], uv: Option<[[f32; 2]; 4]>) {
    let v = |i: usize| {
        let (x, y) = clip(corners[i].0, corners[i].1);
        Vertex {
            position: [x, y],
            colour: colours[i],
            uv: uv.map_or([0.0, 0.0], |u| u[i]),
            textured: if uv.is_some() { 1.0 } else { 0.0 },
        }
    };
    out.extend([v(0), v(1), v(2), v(0), v(2), v(3)]);
}

fn scene() -> Vec<Vertex> {
    let mut v = Vec::new();
    // 1. Background gradient: four different corner colours (interpolation).
    quad(
        &mut v,
        [(0.0, 0.0), (256.0, 0.0), (256.0, 256.0), (0.0, 256.0)],
        [[0.9, 0.1, 0.1, 1.0], [0.1, 0.8, 0.2, 1.0], [0.1, 0.2, 0.9, 1.0], [0.95, 0.9, 0.2, 1.0]],
        None,
    );
    // 2. A fan of thin triangles at awkward angles (edge coverage under MSAA).
    let (cx, cy) = (64.0f32, 64.0f32);
    for i in 0..12 {
        let a = i as f32 * 0.5236 + 0.1;
        let b = a + 0.05;
        let (x0, y0) = clip(cx, cy);
        let (x1, y1) = clip(cx + 60.0 * a.cos(), cy + 60.0 * a.sin());
        let (x2, y2) = clip(cx + 60.0 * b.cos(), cy + 60.0 * b.sin());
        let c = [1.0, 1.0, 1.0, 1.0];
        v.extend([Vertex::flat(x0, y0, c), Vertex::flat(x1, y1, c), Vertex::flat(x2, y2, c)]);
    }
    // 3. Two overlapping half-transparent quads (blending).
    quad(&mut v, [(120.0, 20.5), (230.3, 20.5), (230.3, 110.0), (120.0, 110.0)], [[0.0, 0.0, 0.0, 0.5]; 4], None);
    quad(&mut v, [(160.25, 60.0), (250.0, 60.0), (250.0, 140.75), (160.25, 140.75)], [[1.0, 1.0, 1.0, 0.5]; 4], None);
    // 4. A 4x4 checker magnified to ~97 px at a non-integer offset (bilinear filtering precision).
    quad(
        &mut v,
        [(30.3, 140.7), (127.6, 140.7), (127.6, 238.1), (30.3, 238.1)],
        [[1.0, 1.0, 1.0, 1.0]; 4],
        Some([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
    );
    // 5. A rotated quad with sub-pixel corners (diagonal edges, partial coverage).
    quad(
        &mut v,
        [(190.4, 160.2), (240.9, 195.6), (205.1, 246.3), (154.7, 210.8)],
        [[0.2, 0.9, 0.9, 1.0], [0.9, 0.2, 0.9, 1.0], [0.9, 0.9, 0.2, 1.0], [0.2, 0.2, 0.2, 1.0]],
        None,
    );
    v
}

fn as_bytes(v: &[Vertex]) -> &[u8] {
    // SAFETY: `Vertex` is `repr(C)` and made only of `f32`s, so it has no padding and every byte is
    // initialised; the slice covers exactly `v`'s memory.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn main() -> ExitCode {
    let mut out = String::from("p4.png");
    let mut software = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--out", Some(v)) => out = v,
            ("--adapter", Some(v)) if v == "software" => software = true,
            ("--adapter", Some(v)) if v == "any" => software = false,
            _ => {
                eprintln!("usage: p4-software-parity [--out file.png] [--adapter software|any]");
                return ExitCode::from(2);
            }
        }
    }
    match run(&out, software) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("p4 result=error error={e:?}");
            ExitCode::from(2)
        }
    }
}

fn run(out: &str, software: bool) -> Result<String, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        force_fallback_adapter: software,
        ..Default::default()
    }))
    .map_err(|e| format!("adapter: {e}"))?;
    let info = adapter.get_info();
    if software && info.device_type != wgpu::DeviceType::Cpu {
        return Err(format!("no software adapter (got {:?} {:?})", info.name, info.device_type));
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| format!("device: {e}"))?;

    let extent = wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 };
    let target = |samples, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        })
    };
    let msaa = target(SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let resolved = target(1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);

    // 4x4 checker, two greys, linear (not sRGB) so the filtered values are the raw interpolation.
    let checker = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("checker"),
        size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut texels = Vec::with_capacity(64);
    for y in 0..4 {
        for x in 0..4 {
            let g = if (x + y) % 2 == 0 { 230 } else { 40 };
            texels.extend([g, g, g, 255]);
        }
    }
    queue.write_texture(
        checker.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16), rows_per_image: None },
        wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&checker.create_view(&Default::default())),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2, 3 => Float32],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
        multiview_mask: None,
        cache: None,
    });

    let vertices = scene();
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: std::mem::size_of_val(vertices.as_slice()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, as_bytes(&vertices));

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIZE * SIZE * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let msaa_view = msaa.create_view(&Default::default());
        let resolved_view = resolved.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view,
                depth_slice: None,
                resolve_target: Some(&resolved_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
    encoder.copy_texture_to_buffer(
        resolved.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: None },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("poll: {e}"))?;
    rx.recv().map_err(|e| format!("map channel: {e}"))?.map_err(|e| format!("map: {e}"))?;
    let pixels = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?.to_vec();

    let digest: String = Sha256::digest(&pixels).iter().map(|b| format!("{b:02x}")).collect();
    let distinct = {
        let mut seen: Vec<[u8; 4]> = pixels.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    let file = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&pixels))
        .map_err(|e| format!("png: {e}"))?;

    Ok(format!(
        "p4 result=rendered os={} backend={:?} adapter={:?} device_type={:?} driver={:?} driver_info={:?} \
         size={SIZE} samples={SAMPLES} distinct_colours={distinct} sha256={digest} out={out}",
        std::env::consts::OS,
        info.backend,
        info.name,
        info.device_type,
        info.driver,
        info.driver_info,
    ))
}
