use crate::mesh::{EdgeSegment, Mesh, Vertex};
use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    pub fn rack(dimensions: [f64; 3]) -> Self {
        let size = Vec3::new(
            dimensions[0] as f32,
            dimensions[1] as f32,
            dimensions[2] as f32,
        );
        Self {
            target: size / 2.0,
            distance: size.length() * 1.7,
            yaw: -0.95,
            pitch: 0.55,
        }
    }

    pub fn matrix(&self, aspect: f32) -> Mat4 {
        assert!(aspect.is_finite());
        assert!(aspect > 0.0);
        Mat4::perspective_rh(40_f32.to_radians(), aspect, 0.1, 10000.0)
            * Mat4::look_at_rh(self.eye(), self.target, Vec3::Z)
    }

    pub fn eye(&self) -> Vec3 {
        let direction = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
        );
        self.target + direction * self.distance
    }

    pub fn interact(&mut self, response: &egui::Response, ui: &egui::Ui) {
        let motion = response.drag_motion();
        if response.dragged() {
            if response.dragged_by(egui::PointerButton::Secondary)
                || ui.input(|input| input.modifiers.shift)
            {
                let right = Vec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0);
                let direction = Vec3::new(
                    self.yaw.cos() * self.pitch.cos(),
                    self.yaw.sin() * self.pitch.cos(),
                    self.pitch.sin(),
                );
                let up = direction.cross(right);
                self.target += (-right * motion.x + up * motion.y) * self.distance
                    / response.rect.height().max(1.0)
                    * 0.7;
            } else {
                self.yaw -= motion.x * 0.007;
                self.pitch = (self.pitch + motion.y * 0.007).clamp(-1.55, 1.55);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(10.0, 5000.0);
        }
    }
}

pub struct Draw {
    pub edge_width_px: f32,
    pub edges: Arc<Vec<EdgeSegment>>,
    pub edges_enabled: bool,
    pub eye: Vec3,
    pub viewport_size: [f32; 2],
    pub saved_at: Option<std::time::SystemTime>,
    pub mesh: Arc<Mesh>,
    pub revision: u64,
    pub matrix: Mat4,
    pub selected: [u32; 2],
    pub selected_edge: Option<[u32; 2]>,
}
struct Gpu {
    line_pipeline: wgpu::RenderPipeline,
    lines: wgpu::Buffer,
    line_count: u32,
    upload_ms: f64,
    saved_ms: Option<f64>,
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    revision: u64,
    count: u32,
    pub uploads: u64,
}

pub fn initialize(context: &eframe::CreationContext<'_>) -> Result<(), String> {
    let state = context
        .wgpu_render_state
        .as_ref()
        .ok_or("A wgpu device is required")?;
    let device = &state.device;
    let adapter = state.adapter.get_info();
    eprintln!("Graphics: {} ({:?})", adapter.name, adapter.backend);
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor { label:Some("CAD viewport"),source:wgpu::ShaderSource::Wgsl(r#"
struct Uniforms { matrix:mat4x4<f32>, selection:vec4<f32>, eye:vec4<f32>, edges:vec4<f32> }
@group(0) @binding(0) var<uniform> uniforms:Uniforms;
struct Output { @builtin(position) position:vec4<f32>, @location(0) normal:vec3<f32>, @location(1) @interpolate(flat) face:u32, @location(2) world:vec3<f32>, @location(3) color:vec3<f32> }
@vertex fn vertex_main(@location(0) position:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) face:u32,@location(3) color:vec4<f32>)->Output {
 var output:Output; output.position=uniforms.matrix*vec4<f32>(position,1.0); output.normal=normal; output.face=face; output.world=position; output.color=color.rgb; return output;
}
@fragment fn fragment_main(input:Output)->@location(0) vec4<f32> {
 let normal=normalize(input.normal);
 let view=normalize(uniforms.eye.xyz-input.world);
 let key=normalize(vec3<f32>(-0.35,-0.55,0.85));
 let fill=normalize(vec3<f32>(0.8,0.4,0.3));
 let light=0.24+0.62*max(dot(normal,key),0.0)+0.16*max(dot(normal,fill),0.0);
 let specular=0.20*pow(max(dot(normal,normalize(key+view)),0.0),48.0);
 let rim=0.07*pow(1.0-max(dot(normal,view),0.0),3.0);
 var color=input.color; if input.face==u32(uniforms.selection.x) {color=vec3<f32>(1.0,0.65,0.20);}
 if input.face==u32(uniforms.eye.w) {color=vec3<f32>(0.35,0.72,1.0);}
 return vec4<f32>(color*light+vec3<f32>(specular+rim),1.0);
}
struct LineOutput { @builtin(position) position:vec4<f32>, @location(0) side:f32, @location(1) @interpolate(flat) selected:u32 }
@vertex fn line_vertex(@location(0) start:vec3<f32>,@location(1) end:vec3<f32>,@builtin(vertex_index) index:u32,@builtin(instance_index) segment:u32)->LineOutput {
 let a=uniforms.matrix*vec4<f32>(start,1.0);
 let b=uniforms.matrix*vec4<f32>(end,1.0);
 let viewport=max(uniforms.selection.yz,vec2<f32>(1.0));
 let delta=(b.xy/b.w-a.xy/a.w)*viewport;
 let perpendicular=vec2<f32>(-delta.y,delta.x)/max(length(delta),0.001);
 let corner=array<vec2<f32>,6>(vec2<f32>(0.0,-1.0),vec2<f32>(1.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(0.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(0.0,1.0))[index];
 var clip=mix(a,b,corner.x);
 let selected=segment>=u32(uniforms.edges.x) && segment<u32(uniforms.edges.y);
 let width=select(uniforms.selection.w,max(uniforms.selection.w,3.0),selected);
 clip=vec4<f32>(clip.xy+perpendicular*corner.y*width*clip.w/viewport,clip.zw);
 var output:LineOutput; output.position=clip; output.side=corner.y; output.selected=u32(selected); return output;
}
@fragment fn line_fragment(input:LineOutput)->@location(0) vec4<f32> {
 if uniforms.edges.z==0.0 && input.selected==0u { discard; }
 let color=select(vec3<f32>(0.07,0.08,0.10),vec3<f32>(0.45,0.78,1.0),input.selected==1u);
 return vec4<f32>(color,1.0-smoothstep(0.65,1.0,abs(input.side)));
}
"#.into()) });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Camera"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("CAD"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("CAD"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Uint32,3=>Unorm8x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(state.target_format.into())],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Camera"),
        contents: &[0; 112],
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Camera"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    let vertices = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Empty model"),
        size: 28,
        usage: wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    });
    let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("CAD edge outlines"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("line_vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<EdgeSegment>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("line_fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: state.target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: -2,
                slope_scale: -1.0,
                clamp: 0.0,
            },
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let lines = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Empty CAD edges"),
        size: 24,
        usage: wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    });
    state.renderer.write().callback_resources.insert(Gpu {
        line_pipeline,
        lines,
        line_count: 0,
        upload_ms: 0.0,
        saved_ms: None,
        pipeline,
        group,
        uniform,
        vertices,
        revision: 0,
        count: 0,
        uploads: 0,
    });
    Ok(())
}

impl egui_wgpu::CallbackTrait for Draw {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let gpu = resources
            .get_mut::<Gpu>()
            .expect("Viewport initialized before drawing");
        if gpu.revision != self.revision {
            let started = std::time::Instant::now();
            gpu.vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("CAD triangles"),
                contents: bytemuck::cast_slice(&self.mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            gpu.count = u32::try_from(self.mesh.vertices.len()).expect("Mesh budget checked");
            if !self.edges.is_empty() {
                gpu.lines = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("CAD curve segments"),
                    contents: bytemuck::cast_slice(self.edges.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            }
            gpu.line_count = u32::try_from(self.edges.len()).expect("Edge budget checked");
            gpu.revision = self.revision;
            gpu.uploads += 1;
            gpu.upload_ms = started.elapsed().as_secs_f64() * 1000.0;
            gpu.saved_ms = self
                .saved_at
                .and_then(|saved| std::time::SystemTime::now().duration_since(saved).ok())
                .map(|elapsed| elapsed.as_secs_f64() * 1000.0);
        }
        let mut uniform = [0_f32; 28];
        uniform[..16].copy_from_slice(&self.matrix.to_cols_array());
        uniform[16] = self.selected[0] as f32;
        uniform[17..19].copy_from_slice(&self.viewport_size);
        uniform[19] = self.edge_width_px;
        uniform[20..23].copy_from_slice(&self.eye.to_array());
        uniform[23] = self.selected[1] as f32;
        if let Some([start, end]) = self.selected_edge {
            uniform[24] = start as f32;
            uniform[25] = end as f32;
        }
        uniform[26] = f32::from(self.edges_enabled);
        queue.write_buffer(&gpu.uniform, 0, bytemuck::cast_slice(&uniform));
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let gpu = resources
            .get::<Gpu>()
            .expect("Viewport initialized before drawing");
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.group, &[]);
        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
        pass.draw(0..gpu.count, 0..1);
        if self.edges_enabled || self.selected_edge.is_some() {
            pass.set_pipeline(&gpu.line_pipeline);
            pass.set_vertex_buffer(0, gpu.lines.slice(..));
            pass.draw(0..6, 0..gpu.line_count);
        }
    }
}

pub fn upload_count(frame: &eframe::Frame) -> u64 {
    upload_metrics(frame).0
}

pub fn upload_metrics(frame: &eframe::Frame) -> (u64, f64, Option<f64>) {
    frame
        .wgpu_render_state()
        .and_then(|state| {
            state
                .renderer
                .read()
                .callback_resources
                .get::<Gpu>()
                .map(|gpu| (gpu.uploads, gpu.upload_ms, gpu.saved_ms))
        })
        .unwrap_or((0, 0.0, None))
}
