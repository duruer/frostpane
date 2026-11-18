//! wgpu side: one render pipeline per [`PassKind`] (WGSL ports of the five
//! shaders), per-thread buffer slots and the frame encoder (desktop-native 3.9,
//! misc 5). Works on any `TextureView` of format [`FORMAT`], so tests and the
//! dump example render offscreen with the same code the panels use.

use crate::plan;
use fp_core::gpu::{Blend, DrawList, PassKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Swap chain and offscreen format (BGRA, premultiplied alpha).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

const COMMON: &str = include_str!("shaders/common.wgsl");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Module {
    SnowDrift,
    SnowSprite,
    Rain,
    Petals,
    Sky,
    Fireflies,
}

impl Module {
    fn source(self) -> &'static str {
        match self {
            Module::SnowDrift => include_str!("shaders/snow_drift.wgsl"),
            Module::SnowSprite => include_str!("shaders/snow_sprite.wgsl"),
            Module::Rain => include_str!("shaders/rain.wgsl"),
            Module::Petals => include_str!("shaders/petals.wgsl"),
            Module::Sky => include_str!("shaders/sky.wgsl"),
            Module::Fireflies => include_str!("shaders/fireflies.wgsl"),
        }
    }
}

/// Shader entry points and storage buffer count of a pass kind.
struct Entry {
    module: Module,
    vs: &'static str,
    fs: &'static str,
    storage: usize,
}

fn entry(kind: PassKind) -> Entry {
    let (module, vs, fs, storage) = match kind {
        PassKind::SnowDrift => (Module::SnowDrift, "drift_vs", "drift_fs", 2),
        PassKind::SnowFlakes => (Module::SnowSprite, "flake_vs", "sprite_fs", 1),
        PassKind::SnowLoose => (Module::SnowSprite, "loose_vs", "sprite_fs", 1),
        PassKind::RainFlash => (Module::Rain, "flash_vs", "flash_fs", 0),
        PassKind::RainRipples => (Module::Rain, "ripple_vs", "ripple_fs", 1),
        PassKind::RainStrokes => (Module::Rain, "stroke_vs", "stroke_fs", 1),
        PassKind::Petals => (Module::Petals, "petal_vs", "petal_fs", 1),
        PassKind::StarTrails => (Module::Sky, "trail_vs", "trail_fs", 1),
        PassKind::Stars => (Module::Sky, "star_vs", "star_fs", 1),
        PassKind::Fireflies => (Module::Fireflies, "fly_vs", "fly_fs", 1),
    };
    Entry { module, vs, fs, storage }
}

/// Every pass kind, in [`PassKind`] order.
pub const KINDS: [PassKind; 10] = [
    PassKind::SnowDrift,
    PassKind::SnowFlakes,
    PassKind::SnowLoose,
    PassKind::RainFlash,
    PassKind::RainRipples,
    PassKind::RainStrokes,
    PassKind::Petals,
    PassKind::StarTrails,
    PassKind::Stars,
    PassKind::Fireflies,
];

/// The pass kinds one weather draws, so all of them can be built as soon as
/// the first one shows up.
pub fn family(kind: PassKind) -> &'static [PassKind] {
    match kind {
        PassKind::SnowDrift | PassKind::SnowFlakes | PassKind::SnowLoose => &KINDS[0..3],
        PassKind::RainFlash | PassKind::RainRipples | PassKind::RainStrokes => &KINDS[3..6],
        PassKind::Petals => &KINDS[6..7],
        PassKind::StarTrails | PassKind::Stars | PassKind::Fireflies => &KINDS[7..10],
    }
}

/// Storage buffers a pass kind binds (bindings 2..).
pub fn storage_count(kind: PassKind) -> usize {
    entry(kind).storage
}

fn blend_state(blend: Blend) -> wgpu::BlendState {
    let over = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    match blend {
        Blend::Premultiplied => wgpu::BlendState { color: over, alpha: over },
        Blend::Additive => wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: over,
        },
    }
}

/// The shared device with its pipeline cache. Pipelines are built on first
/// use of a pass kind.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    layouts: Vec<wgpu::BindGroupLayout>,
    pipeline_layouts: Vec<wgpu::PipelineLayout>,
    modules: Mutex<HashMap<Module, wgpu::ShaderModule>>,
    pipelines: Mutex<HashMap<PassKind, Arc<wgpu::RenderPipeline>>>,
    /// First kind of every family whose pipelines are built or being built.
    warmed: Mutex<Vec<PassKind>>,
}

/// Shader-visible descriptors the overlay device reserves. Each pass slot
/// holds at most four buffer bindings, so a few thousand is far more than
/// any frame needs.
pub const DESCRIPTORS: u32 = 4096;

/// Device settings of the overlay: small GPU memory blocks and a small
/// descriptor heap. The defaults (`Performance` memory hint, one million
/// descriptors) commit about 270 MB per device up front; on an integrated GPU
/// that is ordinary process memory. With these the device costs about 30 MB.
/// GL adapters start from the downlevel limits (the full defaults fail there).
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let base = match adapter.get_info().backend {
        wgpu::Backend::Gl => wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        _ => wgpu::Limits::default(),
    };
    wgpu::DeviceDescriptor {
        label: Some("frostpane overlay"),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        required_limits: wgpu::Limits { max_non_sampler_bindings: DESCRIPTORS, ..base },
        ..Default::default()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let vis = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut layouts = Vec::new();
        let mut pipeline_layouts = Vec::new();
        for n in 0..=2u32 {
            let mut entries = vec![uniform(0), uniform(1)];
            entries.extend((0..n).map(|i| storage(2 + i)));
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("overlay bindings"),
                entries: &entries,
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay pipeline layout"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            layouts.push(bgl);
            pipeline_layouts.push(pl);
        }
        Self {
            device,
            queue,
            layouts,
            pipeline_layouts,
            modules: Mutex::new(HashMap::new()),
            pipelines: Mutex::new(HashMap::new()),
            warmed: Mutex::new(Vec::new()),
        }
    }

    /// A device without a surface (tests, the dump example).
    pub fn headless() -> Result<Self, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = fp_platform::NATIVE_BACKENDS;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .or_else(|_| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: true,
                    ..Default::default()
                }))
            })
            .map_err(|e| format!("no adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(&adapter)))
            .map_err(|e| format!("no device: {e}"))?;
        Ok(Self::new(device, queue))
    }

    fn module(&self, m: Module) -> wgpu::ShaderModule {
        let mut cache = lock(&self.modules);
        cache
            .entry(m)
            .or_insert_with(|| {
                let src = format!("{COMMON}\n{}", m.source());
                self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("overlay shader"),
                    source: wgpu::ShaderSource::Wgsl(src.into()),
                })
            })
            .clone()
    }

    /// The pipeline of a pass kind, built on first use.
    /// Builds the pipelines of every family `list` draws on a background
    /// thread, once per family. A pass kind first seen mid-run (the ground
    /// drift once snow piles up, loose snow on the first sweep) otherwise
    /// compiles its shaders on the render thread and drops frames. Only the
    /// families in use are built: all of them cost about 40 MB.
    pub fn warm_families(self: &Arc<Self>, list: &DrawList) {
        let mut new = Vec::new();
        {
            let mut warmed = lock(&self.warmed);
            for pass in &list.passes {
                let fam = family(pass.kind);
                if !warmed.contains(&fam[0]) {
                    warmed.push(fam[0]);
                    new.push(fam);
                }
            }
        }
        if new.is_empty() {
            return;
        }
        let g = self.clone();
        let spawned = std::thread::Builder::new().name("frostpane overlay warm-up".into()).spawn(move || {
            for kind in new.into_iter().flatten() {
                g.pipeline(*kind);
            }
        });
        if let Err(e) = spawned {
            log::warn!("overlay: pipeline warm-up thread failed: {e}");
        }
    }

    pub fn pipeline(&self, kind: PassKind) -> Arc<wgpu::RenderPipeline> {
        if let Some(p) = lock(&self.pipelines).get(&kind) {
            return p.clone();
        }
        let e = entry(kind);
        let module = self.module(e.module);
        let target = wgpu::ColorTargetState {
            format: FORMAT,
            blend: Some(blend_state(kind.blend())),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&self.pipeline_layouts[e.storage]),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(e.vs),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(e.fs),
                compilation_options: Default::default(),
                targets: &[Some(target)],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline = Arc::new(pipeline);
        lock(&self.pipelines).insert(kind, pipeline.clone());
        pipeline
    }

    /// Encodes and submits one frame: clear to transparent, then every pass
    /// of `list` in order. `None` only clears.
    pub fn render(&self, slots: &mut Slots, view: &wgpu::TextureView, list: Option<&DrawList>) {
        let mut jobs: Vec<(Arc<wgpu::RenderPipeline>, u32, u32)> = Vec::new();
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay") });
        if let Some(list) = list {
            slots.begin(&list.scene);
            for pass in &list.passes {
                if pass.vertices == 0 || pass.instances == 0 {
                    continue;
                }
                let need = storage_count(pass.kind);
                if pass.buffers.len() < need {
                    log::warn!("overlay: {:?} pass has {} buffers, needs {need}", pass.kind, pass.buffers.len());
                    continue;
                }
                let idx = jobs.len();
                slots.stage(idx, pass);
                jobs.push((self.pipeline(pass.kind), pass.vertices, pass.instances));
            }
            slots.upload(self, &mut enc, jobs.len());
        }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (idx, (pipeline, vertices, instances)) in jobs.iter().enumerate() {
                rp.set_pipeline(pipeline);
                rp.set_bind_group(0, slots.group(idx), &[]);
                rp.draw(0..*vertices, 0..*instances);
            }
        }
        slots.belt.finish();
        self.queue.submit([enc.finish()]);
        slots.belt.recall();
    }

    /// Renders a list into a fresh `width` x `height` image (BGRA bytes,
    /// premultiplied). For tests and the dump example.
    pub fn render_image(&self, width: u32, height: u32, list: &DrawList) -> Image {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut slots = Slots::new(self);
        self.render(&mut slots, &view, Some(list));
        let row = (width * 4).next_multiple_of(256);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let mapped = slice.get_mapped_range().expect("readback range");
        let mut bgra = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            bgra.extend_from_slice(&mapped[start..start + width as usize * 4]);
        }
        drop(mapped);
        buf.unmap();
        Image { width, height, bgra }
    }
}

/// An offscreen render result.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA, row-major, top row first.
    pub bgra: Vec<u8>,
}

impl Image {
    /// Pixel as premultiplied `[b, g, r, a]`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.bgra[i], self.bgra[i + 1], self.bgra[i + 2], self.bgra[i + 3]]
    }

    /// Number of pixels with alpha above `min`.
    pub fn covered(&self, min: u8) -> usize {
        self.bgra.as_chunks::<4>().0.iter().filter(|p| p[3] > min).count()
    }
}

/// Smallest staging chunk; a frame of heavy snow packs to a few hundred KB.
const STAGING_CHUNK: u64 = 256 * 1024;

struct Slot {
    kind: PassKind,
    /// Reserved bytes per storage binding; grows, never shrinks, so the
    /// packed layout (and the bind group) stays put from frame to frame.
    caps: Vec<u64>,
    /// `(offset, size)` of the uniform block, then of each storage binding,
    /// for the frame being staged.
    want: Vec<(u64, u64)>,
    /// The ranges and upload buffer generation `group` was built for.
    built: Vec<(u64, u64)>,
    built_gen: u64,
    group: Option<wgpu::BindGroup>,
}

/// Per-render-thread GPU data: the scene uniform and one slot per pass index,
/// all packed into one upload buffer that is written once per frame. Reused
/// every frame; grown on demand. Pass `n` of a frame always uses slot `n`,
/// so two passes of one submit never share a range.
pub struct Slots {
    packer: plan::Packer,
    /// Reused mapped staging chunks for the per-frame upload. A plain
    /// `write_buffer` creates and destroys a staging buffer every call, which
    /// was the largest CPU cost of a frame after the driver's own work.
    belt: wgpu::util::StagingBelt,
    upload: Option<(wgpu::Buffer, u64)>,
    /// Bumped whenever the upload buffer is replaced.
    generation: u64,
    scene: (u64, u64),
    uniform_align: u64,
    storage_align: u64,
    slots: Vec<Slot>,
}

impl Slots {
    pub fn new(gpu: &Gpu) -> Self {
        let limits = gpu.device.limits();
        Self {
            packer: plan::Packer::default(),
            belt: wgpu::util::StagingBelt::new(gpu.device.clone(), STAGING_CHUNK),
            upload: None,
            generation: 0,
            scene: (0, 16),
            uniform_align: u64::from(limits.min_uniform_buffer_offset_alignment),
            storage_align: u64::from(limits.min_storage_buffer_offset_alignment),
            slots: Vec::new(),
        }
    }

    /// Starts packing a frame with its scene block.
    fn begin(&mut self, scene: &[f32; 4]) {
        self.packer.clear();
        self.scene = self.packer.place(bytemuck::bytes_of(scene), 16, self.uniform_align);
    }

    /// Packs `pass` for slot `idx` (uniform block, then its storage buffers).
    fn stage(&mut self, idx: usize, pass: &fp_core::gpu::Pass) {
        let need = storage_count(pass.kind);
        while self.slots.len() <= idx {
            self.slots.push(Slot {
                kind: pass.kind,
                caps: Vec::new(),
                want: Vec::new(),
                built: Vec::new(),
                built_gen: 0,
                group: None,
            });
        }
        let slot = &mut self.slots[idx];
        if slot.kind != pass.kind {
            slot.kind = pass.kind;
            slot.group = None;
        }
        let (block, cut) = plan::uniform_block(&pass.uniforms);
        if cut {
            log::warn!("overlay: {:?} uniforms cut to {} floats", pass.kind, plan::UNIFORM_FLOATS);
        }
        slot.want.clear();
        let uniform_size = (plan::UNIFORM_FLOATS * 4) as u64;
        slot.want.push(self.packer.place(bytemuck::cast_slice(&block), uniform_size, self.uniform_align));
        slot.caps.resize(need, 0);
        for (j, data) in pass.buffers.iter().take(need).enumerate() {
            let cap = &mut slot.caps[j];
            *cap = (*cap).max(plan::buffer_capacity(plan::padded_len(data.len())));
            slot.want.push(self.packer.place(data, *cap, self.storage_align));
        }
    }

    /// Writes the packed frame in one go (growing the buffer when needed)
    /// and rebuilds the bind groups of the first `used` slots whose ranges
    /// moved.
    fn upload(&mut self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, used: usize) {
        let bytes = self.packer.bytes();
        let len = bytes.len() as u64;
        if self.upload.as_ref().is_none_or(|(_, cap)| *cap < len) {
            let cap = plan::buffer_capacity(len as usize);
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay frame data"),
                size: cap,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.upload = Some((buf, cap));
            self.generation += 1;
            // Chunks as large as the buffer: the belt never keeps a pile of
            // outgrown small chunks around.
            self.belt = wgpu::util::StagingBelt::new(gpu.device.clone(), cap.max(STAGING_CHUNK));
        }
        let Some((buf, _)) = &self.upload else { return };
        if let Some(size) = wgpu::BufferSize::new(len) {
            self.belt.write_buffer(enc, buf, 0, size).copy_from_slice(bytes);
        }
        let binding = |(offset, size): (u64, u64)| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: buf,
                offset,
                size: wgpu::BufferSize::new(size),
            })
        };
        for slot in self.slots.iter_mut().take(used) {
            if slot.group.is_some() && slot.built_gen == self.generation && slot.built == slot.want {
                continue;
            }
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: binding(self.scene) }];
            for (j, r) in slot.want.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 1 + j as u32, resource: binding(*r) });
            }
            slot.group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("overlay pass"),
                layout: &gpu.layouts[slot.want.len() - 1],
                entries: &entries,
            }));
            slot.built.clone_from(&slot.want);
            slot.built_gen = self.generation;
        }
    }

    fn group(&self, idx: usize) -> &wgpu::BindGroup {
        self.slots[idx].group.as_ref().expect("slot prepared before drawing")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::gpu::*;
    use std::sync::OnceLock;

    const W: u32 = 100;
    const H: u32 = 100;

    /// One shared headless device; `None` (tests skip) on machines without a GPU
    /// for this target's backends.
    fn gpu() -> Option<&'static Gpu> {
        static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
        GPU.get_or_init(|| match Gpu::headless() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("no GPU, overlay render tests skipped: {e}");
                None
            }
        })
        .as_ref()
    }

    fn list() -> DrawList {
        DrawList::new(W as f32, H as f32, 1.0)
    }

    /// Alpha at world point (x, y), Y up.
    fn at(img: &Image, x: u32, y: u32) -> u8 {
        img.pixel(x, H - 1 - y)[3]
    }

    fn floats(v: &[f32]) -> Vec<u8> {
        bytemuck::cast_slice(v).to_vec()
    }

    #[test]
    fn empty_list_is_transparent() {
        let Some(g) = gpu() else { return };
        let img = g.render_image(W, H, &list());
        assert_eq!(img.covered(0), 0);
        assert_eq!((img.width, img.height), (W, H));
    }

    #[test]
    fn rain_strokes_ripples_and_flash() {
        let Some(g) = gpu() else { return };
        let mut d = list();
        d.instanced(
            PassKind::RainStrokes,
            vec![],
            &[RainStroke { head: [50.0, 80.0], tail: [50.0, 60.0], width: 2.0, alpha: 1.0, glow: 0.0, pad: 0.0 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 50, 78) > 100, "head end of the streak is bright");
        assert!(at(&img, 50, 78) > at(&img, 50, 62), "tail is fainter than head");
        assert_eq!(at(&img, 20, 70), 0);

        let mut d = list();
        d.instanced(
            PassKind::RainStrokes,
            vec![],
            &[RainStroke { head: [30.0, 80.0], tail: [30.0, 60.0], width: 2.0, alpha: 1.0, glow: 1.0, pad: 0.0 }],
        );
        assert!(at(&g.render_image(W, H, &d), 30, 70) > 100, "bolt core");

        let mut d = list();
        d.instanced(PassKind::RainRipples, vec![], &[RainRipple { center: [50.0, 50.0], radius: 20.0, alpha: 1.0 }]);
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 70, 50) > 100, "ring side");
        assert_eq!(at(&img, 50, 50), 0, "ring centre is empty");

        let mut d = list();
        d.push(Pass {
            kind: PassKind::RainFlash,
            uniforms: vec![0.5, H as f32],
            buffers: vec![],
            vertices: 4,
            instances: 1,
        });
        let img = g.render_image(W, H, &d);
        let (top, bottom) = (at(&img, 50, H - 1), at(&img, 50, 0));
        assert!(top > bottom && bottom > 0, "flash is brighter at the top: {top} {bottom}");
    }

    #[test]
    fn petals_and_leaves() {
        let Some(g) = gpu() else { return };
        for shape in [0.0, 1.0, 2.0] {
            let mut d = list();
            let s = PetalSprite {
                position: [50.0, 50.0],
                size: 24.0,
                angle: 0.3,
                flip: 1.0,
                alpha: 1.0,
                soft: 0.0,
                tint: 0.2,
                shape,
                pad: 0.0,
            };
            d.instanced(PassKind::Petals, vec![], &[s]);
            let img = g.render_image(W, H, &d);
            assert!(img.covered(128) > 100, "shape {shape} draws a body");
            assert_eq!(at(&img, 2, 2), 0);
            // Premultiplied colour never exceeds alpha.
            assert!(
                img.bgra.as_chunks::<4>().0.iter().all(|p| p[..3].iter().all(|c| u16::from(*c) <= u16::from(p[3]) + 1))
            );
            // Flipped (back face) sprites still draw.
            d.passes.clear();
            d.instanced(PassKind::Petals, vec![], &[PetalSprite { flip: -0.6, soft: 0.5, ..s }]);
            assert!(g.render_image(W, H, &d).covered(10) > 50);
        }
    }

    #[test]
    fn sky_and_fireflies_are_additive() {
        let Some(g) = gpu() else { return };
        let mut d = list();
        d.instanced(
            PassKind::Stars,
            vec![],
            &[StarSprite { position: [50.0, 50.0], size: 8.0, angle: 0.0, alpha: 1.0, hue: 0.5 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 50, 50) > 150, "star core");
        assert!(img.pixel(50, 49)[2] >= img.pixel(50, 49)[0], "warm tint: red >= blue");

        let mut d = list();
        d.instanced(
            PassKind::StarTrails,
            vec![],
            &[StarTrail { head: [80.0, 50.0], tail: [20.0, 50.0], width: 2.0, alpha: 1.0, hue: 0.0 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 78, 50) > at(&img, 40, 50), "head is brighter than the tail");
        assert!(at(&img, 40, 50) > 0);

        let mut d = list();
        d.instanced(
            PassKind::Fireflies,
            vec![],
            &[FireflySprite { position: [50.0, 50.0], glow: 1.0, hue: 0.5, size: 2.0, pad: 0.0 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 50, 50) > 100);
        assert_eq!(at(&img, 2, 2), 0);
    }

    #[test]
    fn snow_sprites_and_flakes() {
        let Some(g) = gpu() else { return };
        let mut d = list();
        d.instanced(
            PassKind::SnowLoose,
            vec![],
            &[SnowSprite { pos: [50.0, 50.0], vel: [0.0, 0.0], size: 8.0, alpha: 1.0, soft: 0.0, lump: 0.0 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 50, 50) > 200);
        assert_eq!(at(&img, 50, 70), 0);
        // A lump with motion smear draws a streak and still covers its centre.
        let mut d = list();
        d.instanced(
            PassKind::SnowLoose,
            vec![],
            &[SnowSprite { pos: [50.0, 50.0], vel: [0.0, -900.0], size: 8.0, alpha: 1.0, soft: 0.25, lump: 0.5 }],
        );
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 50, 50) > 100);
        assert!(at(&img, 50, 58) > 0, "streak trails above the head");

        // Closed form: top 90, fall 40, age 0.5, no sway -> y near 70.
        let flake = SnowFlake {
            x: 40.0,
            top: 90.0,
            born: 0.0,
            fall: 40.0,
            drift: 0.0,
            wind_at_birth: 0.0,
            phase: 0.0,
            sway_rate: 1.0,
            sway: 0.0,
            size: 6.0,
            alpha: 1.0,
            soft: 0.5,
            gone: 1e9,
        };
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 0.0, 0.0, W as f32, 30.0], &[flake]);
        let img = g.render_image(W, H, &d);
        let y = (90.0 - 40.0 * (0.5 + 0.12 / 1.3 * (0.0f32.cos() - (1.3f32 * 0.5).cos()))) as u32;
        // The head (lower end of the smear) is the bright part: peak within a pixel of y.
        let peak = at(&img, 40, y).max(at(&img, 40, y - 1));
        assert!(peak > 150, "flake at its closed-form position y={y}");
        // Landed flakes vanish once `now >= gone`.
        let gone = SnowFlake { gone: 0.4, ..flake };
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 0.0, 0.0, W as f32, 30.0], &[gone]);
        assert_eq!(g.render_image(W, H, &d).covered(0), 0);
        // Wind displacement moves the flake in x, wrapping inside the span.
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 0.0, 0.0, W as f32, 30.0], &[SnowFlake { drift: 1.0, ..flake }]);
        let a = g.render_image(W, H, &d);
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 20.0, 0.0, W as f32, 30.0], &[SnowFlake { drift: 1.0, ..flake }]);
        let b = g.render_image(W, H, &d);
        assert!(at(&a, 40, y) > 100 && at(&b, 60, y) > 100 && at(&b, 40, y) == 0);
        // Wrap uses the margin uniform: x = 85 stays with margin 10 and wraps
        // to -15 (off screen) with margin 30.
        let edge = SnowFlake { x: 85.0, ..flake };
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 0.0, 0.0, W as f32, 10.0], &[edge]);
        assert!(at(&g.render_image(W, H, &d), 85, y) > 100, "margin 10 keeps the flake");
        let mut d = list();
        d.instanced(PassKind::SnowFlakes, vec![0.5, 0.0, 0.0, W as f32, 30.0], &[edge]);
        assert_eq!(g.render_image(W, H, &d).covered(0), 0, "margin 30 wraps it off screen");
    }

    #[test]
    fn zero_width_trail_stays_finite() {
        let Some(g) = gpu() else { return };
        let mut d = list();
        d.instanced(
            PassKind::StarTrails,
            vec![],
            &[StarTrail { head: [80.0, 50.0], tail: [20.0, 50.0], width: 0.0, alpha: 1.0, hue: 0.0 }],
        );
        let img = g.render_image(W, H, &d);
        // No NaN blow-up: nothing far from the segment lights up.
        assert_eq!(at(&img, 50, 90), 0);
        assert_eq!(at(&img, 50, 10), 0);
    }

    fn drift_pass(occluders: &[f32]) -> Pass {
        let n = 50usize;
        let mut cols = vec![10.0f32; n];
        cols.extend(vec![0.0f32; n]);
        let occ = if occluders.is_empty() { vec![0.0; 4] } else { occluders.to_vec() };
        Pass {
            kind: PassKind::SnowDrift,
            uniforms: vec![0.0, 0.0, 2.0, n as f32, -1.0, 12.0, (occluders.len() / 4) as f32, 0.0],
            buffers: vec![floats(&cols), floats(&occ)],
            vertices: 4,
            instances: 1,
        }
    }

    #[test]
    fn snow_drift_height_field() {
        let Some(g) = gpu() else { return };
        let mut d = list();
        d.push(drift_pass(&[]));
        let img = g.render_image(W, H, &d);
        assert!(at(&img, 20, 5) > 250, "inside the drift");
        assert_eq!(at(&img, 20, 20), 0, "above the surface");
        // Occluders (windows in front) cut the drift out.
        let mut d = list();
        d.push(drift_pass(&[40.0, 0.0, 60.0, 30.0]));
        let img = g.render_image(W, H, &d);
        assert_eq!(at(&img, 50, 5), 0);
        assert!(at(&img, 20, 5) > 250 && at(&img, 70, 5) > 250);
    }

    #[test]
    fn slots_grow_and_switch_kind() {
        let Some(g) = gpu() else { return };
        let mut slots = Slots::new(g);
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let sprite = SnowSprite { pos: [50.0, 50.0], vel: [0.0; 2], size: 4.0, alpha: 1.0, soft: 0.0, lump: 0.0 };
        let mut d = list();
        // Frame 1: one sprite pass in slot 0, one ripple pass in slot 1.
        d.instanced(PassKind::SnowLoose, vec![], &[sprite]);
        d.instanced(PassKind::RainRipples, vec![], &[RainRipple::default()]);
        g.render(&mut slots, &view, Some(&d));
        // Frame 2: slot 0 switches kind and the later slots grow past their capacity.
        let mut d = list();
        d.instanced(PassKind::RainRipples, vec![], &[RainRipple::default(); 100]);
        d.instanced(PassKind::SnowLoose, vec![], &vec![sprite; 5000]);
        d.push(drift_pass(&[]));
        g.render(&mut slots, &view, Some(&d));
        // Short buffer lists and empty passes are skipped, a missing list clears.
        let mut d = list();
        d.push(Pass {
            kind: PassKind::SnowDrift,
            uniforms: vec![],
            buffers: vec![vec![0; 16]],
            vertices: 4,
            instances: 1,
        });
        d.push(Pass {
            kind: PassKind::Petals,
            uniforms: vec![],
            buffers: vec![vec![0; 40]],
            vertices: 4,
            instances: 0,
        });
        d.push(Pass {
            kind: PassKind::Petals,
            uniforms: vec![0.0; 40],
            buffers: vec![vec![0; 41]],
            vertices: 0,
            instances: 1,
        });
        g.render(&mut slots, &view, Some(&d));
        g.render(&mut slots, &view, None);
        // Odd byte lengths are padded for the copy.
        let mut d = list();
        d.push(Pass {
            kind: PassKind::Fireflies,
            uniforms: vec![],
            buffers: vec![vec![0; 25]],
            vertices: 4,
            instances: 1,
        });
        g.render(&mut slots, &view, Some(&d));
    }

    #[test]
    fn families_cover_every_kind_once() {
        for kind in KINDS {
            assert!(family(kind).contains(&kind));
        }
        let firsts = KINDS.iter().filter(|k| family(**k)[0] == **k).count();
        assert_eq!(firsts, 4);
    }

    #[test]
    fn warm_families_builds_a_family_once() {
        // Its own device: the shared test device must not race other tests.
        let Ok(own) = Gpu::headless().map(Arc::new) else { return };
        let mut d = list();
        d.instanced(PassKind::RainRipples, vec![], &[RainRipple::default()]);
        own.warm_families(&d);
        own.warm_families(&d);
        assert_eq!(*lock(&own.warmed), vec![PassKind::RainFlash]);
        // The background thread builds the whole rain family.
        for _ in 0..500 {
            if lock(&own.pipelines).len() == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let built = lock(&own.pipelines);
        assert!(family(PassKind::RainFlash).iter().all(|k| built.contains_key(k)));
    }

    #[test]
    fn entries_cover_every_kind() {
        let kinds = KINDS;
        assert_eq!(kinds.iter().map(|k| storage_count(*k)).sum::<usize>(), 10);
        assert_eq!(blend_state(kinds[0].blend()).color.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend_state(PassKind::Stars.blend()).color.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(blend_state(PassKind::Stars.blend()).alpha.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
    }
}
