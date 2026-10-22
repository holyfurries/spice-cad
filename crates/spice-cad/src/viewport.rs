use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
pub struct Camera {
    pub orthographic: bool,
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    pub fn frame(&mut self, bounds: [Vec3; 2], aspect: f32) {
        assert!(bounds.iter().all(|point| point.is_finite()));
        assert!(aspect.is_finite());
        assert!(aspect > 0.0);
        assert!(bounds[1].cmpge(bounds[0]).all());
        self.target = (bounds[0] + bounds[1]) * 0.5;
        self.distance =
            ((bounds[1] - bounds[0]).length() * 1.7 / aspect.min(1.0)).clamp(10.0, 5000.0);
    }

    pub fn for_dimensions(dimensions: [f64; 3]) -> Self {
        let size = Vec3::new(
            dimensions[0] as f32,
            dimensions[1] as f32,
            dimensions[2] as f32,
        );
        Self {
            orthographic: false,
            target: size / 2.0,
            distance: size.length() * 1.7,
            yaw: -0.95,
            pitch: 0.55,
        }
    }

    pub fn matrix(&self, aspect: f32) -> Mat4 {
        assert!(aspect.is_finite());
        assert!(aspect > 0.0);
        let projection = if self.orthographic {
            let half_height = self.distance * 20_f32.to_radians().tan();
            Mat4::orthographic_rh(
                -half_height * aspect,
                half_height * aspect,
                -half_height,
                half_height,
                0.1,
                10000.0,
            )
        } else {
            Mat4::perspective_rh(40_f32.to_radians(), aspect, 0.1, 10000.0)
        };
        let up = if self.pitch.abs() > 1.56 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        projection * Mat4::look_at_rh(self.eye(), self.target, up)
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
        let motion = response.drag_delta();
        if response.dragged() {
            if response.dragged_by(egui::PointerButton::Secondary)
                || ui.input(|input| input.modifiers.shift)
                || self.orthographic
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

pub struct Views {
    pub four: bool,
    target: Vec3,
    distance: f32,
}

impl Views {
    pub fn new(camera: Camera) -> Self {
        Self {
            four: false,
            target: camera.target,
            distance: camera.distance,
        }
    }

    pub fn frame(&mut self, camera: Camera) {
        self.target = camera.target;
        self.distance = camera.distance;
    }

    pub fn camera(&self, index: usize, perspective: Camera) -> Camera {
        assert!(index < 4);
        if index == 0 {
            return perspective;
        }
        let (yaw, pitch) = match index {
            1 => (-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
            2 => (-std::f32::consts::FRAC_PI_2, 0.0),
            3 => (std::f32::consts::PI, 0.0),
            _ => unreachable!(),
        };
        Camera {
            target: self.target,
            distance: self.distance,
            yaw,
            pitch,
            orthographic: true,
        }
    }

    pub fn rectangles(rect: egui::Rect, four: bool) -> [Option<egui::Rect>; 4] {
        if !four {
            return [Some(rect), None, None, None];
        }
        let center = rect.center();
        [
            Some(egui::Rect::from_min_max(
                center + egui::vec2(1.0, 1.0),
                rect.max,
            )),
            Some(egui::Rect::from_min_max(
                rect.min,
                center - egui::vec2(1.0, 1.0),
            )),
            Some(egui::Rect::from_min_max(
                egui::pos2(rect.left(), center.y + 1.0),
                egui::pos2(center.x - 1.0, rect.bottom()),
            )),
            Some(egui::Rect::from_min_max(
                egui::pos2(center.x + 1.0, rect.top()),
                egui::pos2(rect.right(), center.y - 1.0),
            )),
        ]
    }
}

pub struct Draw {
    pub view_index: usize,
    pub visible_parts: u32,
    pub input: Option<crate::performance::Input>,
    pub edge_width_px: f32,
    pub edges_enabled: bool,
    pub eye: Vec3,
    pub viewport_size: [f32; 2],
    pub saved_at: Option<std::time::SystemTime>,
    pub geometry: Arc<Geometry>,
    pub instances: Arc<Vec<Instance>>,
    pub matrix: Mat4,
    pub selected: [u32; 2],
    pub selected_parts: u32,
    pub selected_edge: Option<[u32; 2]>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    face: u32,
    part: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Segment {
    start: [f32; 3],
    end: [f32; 3],
    part: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    matrix: [f32; 16],
    color: [f32; 4],
    surface: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Vertex>() == 32);
const _: () = assert!(std::mem::size_of::<Segment>() == 28);
const _: () = assert!(std::mem::size_of::<Instance>() == 96);

impl Instance {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_cols_array(&self.matrix)
    }

    pub fn color(&self) -> [f32; 3] {
        [self.color[0], self.color[1], self.color[2]]
    }

    pub fn new(matrix: Mat4, material: &crate::material::Material) -> Self {
        let color = material.base_color;
        assert!(matrix.is_finite());
        assert!(color.iter().all(|value| value.is_finite()));
        Self {
            matrix: matrix.to_cols_array(),
            color: [color[0], color[1], color[2], 1.0],
            surface: [
                material.specular_roughness,
                material.base_metalness,
                material.effective_ior(),
                0.0,
            ],
        }
    }
}

pub struct Geometry {
    vertices: Vec<Vertex>,
    segments: Vec<Segment>,
    pub parts: Vec<crate::picking::Part>,
}

impl Geometry {
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn face_count(&self) -> u32 {
        *self.parts.last().expect("Nonempty geometry").faces.end()
    }
    pub fn new(parts: &[Arc<crate::model::Geometry>]) -> Result<Self, String> {
        let vertices = parts
            .iter()
            .map(|part| part.mesh.vertices.len())
            .sum::<usize>();
        let segments = parts
            .iter()
            .map(|part| part.edges.segments.len())
            .sum::<usize>();
        if parts.is_empty() || parts.len() > 32 || vertices > 300000 || segments > 131072 {
            return Err("GPU geometry exceeds its part, triangle or edge budget".into());
        }
        let mut result = Self {
            vertices: Vec::with_capacity(vertices),
            segments: Vec::with_capacity(segments),
            parts: Vec::with_capacity(parts.len()),
        };
        let mut faces = 0;
        for (part, geometry) in parts.iter().enumerate() {
            let mesh = &geometry.mesh;
            let segments = &geometry.edges.segments;
            let offset = result.segments.len() as u32;
            result.parts.push(crate::picking::Part {
                geometry: Arc::clone(geometry),
                faces: faces + 1..=faces + mesh.face_count,
                segments: offset..offset + segments.len() as u32,
                mesh_bounds: mesh.vertices.iter().fold(
                    [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
                    |bounds, vertex| {
                        let point = Vec3::from_array(vertex.position);
                        [bounds[0].min(point), bounds[1].max(point)]
                    },
                ),
            });
            result
                .vertices
                .extend(mesh.vertices.iter().map(|vertex| Vertex {
                    position: vertex.position,
                    normal: vertex.normal,
                    face: vertex.face_id + faces,
                    part: part as u32,
                }));
            result
                .segments
                .extend(segments.iter().map(|segment| Segment {
                    start: segment.start,
                    end: segment.end,
                    part: part as u32,
                }));
            faces += mesh.face_count;
        }
        Ok(result)
    }
}
struct Gpu {
    scene: GpuScene,
    line_pipeline: wgpu::RenderPipeline,
    pipeline: wgpu::RenderPipeline,
    cameras: [CameraGpu; 4],
    instances: wgpu::Buffer,
    instances_previous: Option<Arc<Vec<Instance>>>,
    metrics: Metrics,
}

struct CameraGpu {
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    previous: Option<[u32; 32]>,
}

#[derive(Default)]
struct GpuScene {
    vertices: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    geometry: Option<Arc<Geometry>>,
    count: u32,
    line_count: u32,
}

#[derive(Clone, Copy, Default)]
pub struct Metrics {
    pub input_latency: Option<crate::performance::Latency>,
    pub uploads: u64,
    pub buffer_allocations: u64,
    pub uniform_writes: u64,
    pub geometry_bytes: u64,
    pub upload_ms: f64,
    pub saved_ms: Option<f64>,
    pub placement_writes: u64,
    pub placement_bytes: u64,
}

fn upload_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut Option<wgpu::Buffer>,
    bytes: &[u8],
    bytes_max: u64,
    label: &str,
) -> bool {
    let required = bytes.len() as u64;
    assert!(required <= bytes_max);
    assert!(required.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT));
    if bytes.is_empty() {
        return false;
    }
    let allocate = buffer
        .as_ref()
        .is_none_or(|buffer| buffer.size() < required);
    if allocate {
        *buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: required.next_power_of_two().min(bytes_max),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    queue.write_buffer(
        buffer.as_ref().expect("Allocated geometry buffer"),
        0,
        bytes,
    );
    allocate
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
struct Uniforms { matrix:mat4x4<f32>, selection:vec4<f32>, eye:vec4<f32>, edges:vec4<f32>, part:vec4<f32> }
@group(0) @binding(0) var<uniform> uniforms:Uniforms;
struct Instance { matrix:mat4x4<f32>, color:vec4<f32>, surface:vec4<f32> }
@group(0) @binding(1) var<uniform> instances:array<Instance,32>;
fn part_visible(part:u32)->bool {
 let mask=u32(uniforms.part.z)|(u32(uniforms.part.w)<<16u);
 return (mask&(1u<<part))!=0u;
}
struct Output { @builtin(position) position:vec4<f32>, @location(0) normal:vec3<f32>, @location(1) @interpolate(flat) face:u32, @location(2) world:vec3<f32>, @location(3) color:vec3<f32>, @location(4) @interpolate(flat) part:u32 }
@vertex fn vertex_main(@location(0) position:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) face:u32,@location(3) part:u32)->Output {
 let instance=instances[part]; let world=instance.matrix*vec4<f32>(position,1.0);
 var output:Output; output.position=uniforms.matrix*world; output.normal=(instance.matrix*vec4<f32>(normal,0.0)).xyz; output.face=face; output.world=world.xyz; output.color=instance.color.rgb; output.part=part;
 if !part_visible(part) {output.position=vec4<f32>(0.0,0.0,2.0,1.0);} return output;
}
fn dielectric_fresnel(cosine:f32, ior:f32)->f32 {
 if ior==1.0 {return 0.0;}
 let c=clamp(cosine,0.0,1.0); let transmitted=sqrt(max(1.0-(1.0-c*c)/(ior*ior),0.0));
 let perpendicular=(c-ior*transmitted)/max(c+ior*transmitted,0.00000001);
 let parallel=(ior*c-transmitted)/max(ior*c+transmitted,0.00000001);
 return 0.5*(perpendicular*perpendicular+parallel*parallel);
}
fn illuminate(base:vec3<f32>, surface:vec3<f32>, normal:vec3<f32>, view:vec3<f32>, light:vec3<f32>)->vec3<f32> {
 let nl=max(dot(normal,light),0.0); let nv=max(dot(normal,view),0.001);
 let halfway=light+view; let half_vector=halfway*inverseSqrt(max(dot(halfway,halfway),0.00000001));
 let nh=max(dot(normal,half_vector),0.0); let vh=max(dot(view,half_vector),0.0);
 let alpha=pow(max(surface.x,0.045),2.0); let alpha_squared=alpha*alpha;
 let denominator=nh*nh*(alpha_squared-1.0)+1.0;
 let distribution=alpha_squared/max(3.14159265*denominator*denominator,0.000000000001);
 let visibility=0.5/max(nl*sqrt(nv*nv*(1.0-alpha_squared)+alpha_squared)+nv*sqrt(nl*nl*(1.0-alpha_squared)+alpha_squared),0.00001);
 let fresnel_weight=pow(1.0-vh,5.0);
 let dielectric=dielectric_fresnel(vh,surface.z);
 let metal=base+(vec3<f32>(1.0)-base)*fresnel_weight;
 let fresnel=mix(vec3<f32>(dielectric),metal,surface.y);
 let diffuse=(1.0-dielectric)*(1.0-surface.y)*base/3.14159265;
 return (diffuse+distribution*visibility*fresnel)*nl;
}
fn surface_color(input:Output)->vec3<f32> {
 let normal=normalize(input.normal);
 let view=normalize(uniforms.eye.xyz-input.world);
 let key=normalize(vec3<f32>(-0.35,-0.55,0.85));
 let fill=normalize(vec3<f32>(0.8,0.4,0.3));
 let surface=instances[input.part].surface.xyz;
 let selected_parts=u32(uniforms.part.x)|(u32(uniforms.part.y)<<16u);
 var color=input.color; if (selected_parts&(1u<<input.part))!=0u {color=vec3<f32>(1.0,0.65,0.20);}
 if input.face==u32(uniforms.selection.x) {color=vec3<f32>(1.0,0.65,0.20);}
 if input.face==u32(uniforms.eye.w) {color=vec3<f32>(0.35,0.72,1.0);}
 let ambient=color*0.20*(1.0-0.6*surface.y);
 return ambient+3.0*illuminate(color,surface,normal,view,key)+0.8*illuminate(color,surface,normal,view,fill);
}
@fragment fn fragment_main(input:Output)->@location(0) vec4<f32> {
 let linear=clamp(surface_color(input),vec3<f32>(0.0),vec3<f32>(1.0));
 let gamma=select(1.055*pow(linear,vec3<f32>(1.0/2.4))-vec3<f32>(0.055),12.92*linear,linear<=vec3<f32>(0.0031308));
 return vec4<f32>(gamma,1.0);
}
@fragment fn fragment_linear(input:Output)->@location(0) vec4<f32> {
 return vec4<f32>(surface_color(input),1.0);
}
struct LineOutput { @builtin(position) position:vec4<f32>, @location(0) side:f32, @location(1) @interpolate(flat) selected:u32 }
@vertex fn line_vertex(@location(0) start:vec3<f32>,@location(1) end:vec3<f32>,@location(2) part:u32,@builtin(vertex_index) index:u32,@builtin(instance_index) segment:u32)->LineOutput {
 let transform=uniforms.matrix*instances[part].matrix;
 let a=transform*vec4<f32>(start,1.0);
 let b=transform*vec4<f32>(end,1.0);
 let viewport=max(uniforms.selection.yz,vec2<f32>(1.0));
 let delta=(b.xy/b.w-a.xy/a.w)*viewport;
 let perpendicular=vec2<f32>(-delta.y,delta.x)/max(length(delta),0.001);
 let corner=array<vec2<f32>,6>(vec2<f32>(0.0,-1.0),vec2<f32>(1.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(0.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(0.0,1.0))[index];
 var clip=mix(a,b,corner.x);
 let selected=segment>=u32(uniforms.edges.x) && segment<u32(uniforms.edges.y);
 let width=select(uniforms.selection.w,max(uniforms.selection.w,3.0),selected);
 clip=vec4<f32>(clip.xy+perpendicular*corner.y*width*clip.w/viewport,clip.zw);
 var output:LineOutput; output.position=clip; output.side=corner.y; output.selected=u32(selected);
 if !part_visible(part) {output.position=vec4<f32>(0.0,0.0,2.0,1.0);} return output;
}
@fragment fn line_fragment(input:LineOutput)->@location(0) vec4<f32> {
 if uniforms.edges.z==0.0 && input.selected==0u { discard; }
 let color=select(vec3<f32>(0.07,0.08,0.10),vec3<f32>(0.45,0.78,1.0),input.selected==1u);
 return vec4<f32>(color,1.0-smoothstep(0.65,1.0,abs(input.side)));
}
"#.into()) });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Camera"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
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
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Uint32,3=>Uint32],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(if state.target_format.is_srgb() { "fragment_linear" } else { "fragment_main" }),
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
    let instances = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("CAD part placements"),
        size: (32 * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let cameras = std::array::from_fn(|_| {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("CAD camera"),
            contents: &[0; 128],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
            ],
        });
        CameraGpu {
            group,
            uniform,
            previous: None,
        }
    });
    let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("CAD edge outlines"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("line_vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Segment>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Uint32],
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
    state.renderer.write().callback_resources.insert(Gpu {
        scene: Default::default(),
        line_pipeline,
        pipeline,
        cameras,
        instances,
        instances_previous: None,
        metrics: Metrics::default(),
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
        assert!(self.view_index < gpu.cameras.len());
        assert_eq!(self.instances.len(), self.geometry.parts.len());
        assert!(self.instances.len() <= 32);
        let scene = &mut gpu.scene;
        if scene
            .geometry
            .as_ref()
            .is_none_or(|geometry| !Arc::ptr_eq(geometry, &self.geometry))
        {
            let started = std::time::Instant::now();
            gpu.metrics.buffer_allocations += u64::from(upload_buffer(
                device,
                queue,
                &mut scene.vertices,
                bytemuck::cast_slice(&self.geometry.vertices),
                300000 * std::mem::size_of::<Vertex>() as u64,
                "CAD triangles",
            ));
            gpu.metrics.buffer_allocations += u64::from(upload_buffer(
                device,
                queue,
                &mut scene.lines,
                bytemuck::cast_slice(self.geometry.segments.as_slice()),
                131072 * std::mem::size_of::<Segment>() as u64,
                "CAD curve segments",
            ));
            scene.count = u32::try_from(self.geometry.vertices.len()).expect("Mesh budget checked");
            scene.line_count =
                u32::try_from(self.geometry.segments.len()).expect("Edge budget checked");
            scene.geometry = Some(Arc::clone(&self.geometry));
            gpu.metrics.uploads += 1;
            gpu.metrics.upload_ms = started.elapsed().as_secs_f64() * 1000.0;
            gpu.metrics.saved_ms = self
                .saved_at
                .and_then(|saved| std::time::SystemTime::now().duration_since(saved).ok())
                .map(|elapsed| elapsed.as_secs_f64() * 1000.0);
        }
        if gpu.instances_previous.as_ref().is_none_or(|previous| {
            !Arc::ptr_eq(previous, &self.instances)
                && bytemuck::cast_slice::<_, u8>(previous.as_slice())
                    != bytemuck::cast_slice::<_, u8>(self.instances.as_slice())
        }) {
            let bytes = bytemuck::cast_slice(self.instances.as_slice());
            queue.write_buffer(&gpu.instances, 0, bytes);
            gpu.metrics.placement_writes += 1;
            gpu.metrics.placement_bytes += bytes.len() as u64;
        }
        gpu.instances_previous = Some(Arc::clone(&self.instances));
        let mut uniform = [0_f32; 32];
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
        uniform[28] = (self.selected_parts & 0xffff) as f32;
        uniform[29] = (self.selected_parts >> 16) as f32;
        uniform[30] = (self.visible_parts & 0xffff) as f32;
        uniform[31] = (self.visible_parts >> 16) as f32;
        let bits = uniform.map(f32::to_bits);
        let camera = &mut gpu.cameras[self.view_index];
        if camera.previous != Some(bits) {
            queue.write_buffer(&camera.uniform, 0, bytemuck::cast_slice(&uniform));
            camera.previous = Some(bits);
            gpu.metrics.uniform_writes += 1;
        }
        if let Some(input) = self.input {
            if gpu
                .metrics
                .input_latency
                .is_none_or(|latency| latency.revision != input.revision)
            {
                gpu.metrics.input_latency = Some(input.prepared(std::time::Instant::now()));
            }
        }
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
        let scene = &gpu.scene;
        assert!(
            scene
                .geometry
                .as_ref()
                .is_some_and(|geometry| Arc::ptr_eq(geometry, &self.geometry))
        );
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.cameras[self.view_index].group, &[]);
        if scene.count > 0 {
            pass.set_vertex_buffer(
                0,
                scene
                    .vertices
                    .as_ref()
                    .expect("Uploaded triangles")
                    .slice(..),
            );
            pass.draw(0..scene.count, 0..1);
        }
        if scene.line_count > 0 && (self.edges_enabled || self.selected_edge.is_some()) {
            pass.set_pipeline(&gpu.line_pipeline);
            pass.set_vertex_buffer(0, scene.lines.as_ref().expect("Uploaded edges").slice(..));
            pass.draw(0..6, 0..scene.line_count);
        }
    }
}

pub fn metrics(frame: &eframe::Frame) -> Metrics {
    frame
        .wgpu_render_state()
        .and_then(|state| {
            state
                .renderer
                .read()
                .callback_resources
                .get::<Gpu>()
                .map(|gpu| Metrics {
                    geometry_bytes: [&gpu.scene.vertices, &gpu.scene.lines]
                        .into_iter()
                        .flatten()
                        .map(wgpu::Buffer::size)
                        .sum(),
                    ..gpu.metrics
                })
        })
        .unwrap_or_default()
}

pub fn geometry_prepared(frame: &eframe::Frame, geometry: &Arc<Geometry>) -> bool {
    frame.wgpu_render_state().is_some_and(|state| {
        state
            .renderer
            .read()
            .callback_resources
            .get::<Gpu>()
            .is_some_and(|gpu| {
                gpu.scene
                    .geometry
                    .as_ref()
                    .is_some_and(|prepared| Arc::ptr_eq(prepared, geometry))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orthographic_pan_follows_screen_delta_instead_of_raw_mouse_units() {
        let context = egui::Context::default();
        let mut camera = Camera {
            orthographic: true,
            target: Vec3::ZERO,
            distance: 100.0,
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: std::f32::consts::FRAC_PI_2,
        };
        let mut frame = |events| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0)),
                        egui::Id::unique("test_camera_pan"),
                        egui::Sense::click_and_drag(),
                    );
                    camera.interact(&response, ui);
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))]);
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(100.0, 100.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(120.0, 110.0)),
            egui::Event::MouseMoved(egui::vec2(1000.0, 1000.0)),
        ]);
        assert!((camera.target.x + 3.5).abs() < 0.001, "{:?}", camera.target);
        assert!((camera.target.y - 1.75).abs() < 0.001);
        assert_eq!(camera.distance, 100.0);
        assert_eq!(camera.pitch, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn four_views_have_exact_axes_shared_scale_and_correct_picking() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(
            "cad.show('box',cad.box({width=10,depth=20,height=30}))",
            1,
        )?;
        let mut camera = Camera::for_dimensions([10.0, 20.0, 30.0]);
        camera.frame(snapshot.scene.bounds, 1.0);
        let mut views = Views::new(camera);
        let geometry = Arc::clone(&snapshot.scene.display);
        let instances = Arc::clone(&snapshot.scene.instances);
        let mut scale: Option<f32> = None;
        for index in 1..4 {
            let camera = views.camera(index, camera);
            let matrix = camera.matrix(1.0);
            assert!(matrix.is_finite());
            let hit = snapshot
                .scene
                .picking()
                .pick(matrix, [0.0, 0.0])
                .ok_or("Orthographic center missed the model")?;
            let expected = match index {
                1 => 30.0,
                2 | 3 => 0.0,
                _ => unreachable!(),
            };
            let axis = match index {
                1 => 2,
                2 => 1,
                3 => 0,
                _ => unreachable!(),
            };
            assert!(
                (hit.position[axis] - expected).abs() < 0.001,
                "view {index}: {:?}",
                hit.position
            );
            let horizontal = if index == 3 { Vec3::Y } else { Vec3::X };
            let center = matrix.project_point3(camera.target);
            let projected = matrix.project_point3(camera.target + horizontal * 10.0);
            let width = (projected.x - center.x).abs();
            if let Some(expected) = scale {
                assert!((width - expected).abs() < 1e-6);
            }
            scale = Some(width);
            let depth = matrix.project_point3(
                camera.target
                    + horizontal * 10.0
                    + (camera.eye() - camera.target).normalize() * 5.0,
            );
            assert!((depth.x - projected.x).abs() < 1e-5);
        }
        let mut top = views.camera(1, camera);
        top.distance *= 0.5;
        top.target += Vec3::new(2.0, 3.0, 4.0);
        views.frame(top);
        for index in 1..4 {
            let linked = views.camera(index, camera);
            assert_eq!(linked.target, top.target);
            assert_eq!(linked.distance, top.distance);
        }
        assert_eq!(views.camera(0, camera).target, camera.target);
        assert!(Arc::ptr_eq(&geometry, &snapshot.scene.display));
        assert!(Arc::ptr_eq(&instances, &snapshot.scene.instances));
        Ok(())
    }

    #[test]
    fn four_view_rectangles_are_disjoint_and_keep_single_view_full_size() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(801.0, 601.0));
        assert_eq!(
            Views::rectangles(rect, false),
            [Some(rect), None, None, None]
        );
        let panes = Views::rectangles(rect, true).map(|pane| pane.expect("Four views"));
        for first in 0..4 {
            assert!(rect.contains_rect(panes[first]));
            for second in first + 1..4 {
                assert!(!panes[first].intersects(panes[second]));
            }
            assert!((panes[first].height() - panes[0].height()).abs() < 0.001);
        }
    }

    #[test]
    fn gpu_instances_match_cpu_positions_normals_edges_and_face_ids_in_both_layouts()
    -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=20,height=30});local b=cad.cylinder({radius=5,height=15});cad.show('a',a,{material = cad.material('Surface', {base_color = {r=1,g=0,b=0}}),assembly={rotation={x=25,y=40,z=10},position={x=40,y=-10}},print={position={x=20}}});cad.show('b',b,{material = cad.material('Surface', {base_color = {r=0,g=1,b=0}}),assembly={rotation={y=90},position={z=20}},print={rotation={x=90},position={y=30,z=5}}})";
        let mut snapshot = crate::build_script_snapshot(source, 1)?;
        let geometry = Arc::clone(&snapshot.scene.display);
        for layout in [
            crate::placement::Layout::Assembly,
            crate::placement::Layout::Print,
        ] {
            snapshot.set_layout(layout);
            let scene = &snapshot.scene;
            assert!(Arc::ptr_eq(&geometry, &scene.display));
            let mesh = scene.world_mesh();
            let edges = scene.world_edges();
            assert_eq!(geometry.vertices.len(), mesh.vertices.len());
            assert_eq!(geometry.segments.len(), edges.segments.len());
            for (vertex, expected) in geometry.vertices.iter().zip(&mesh.vertices) {
                let instance = scene.instances[vertex.part as usize];
                let matrix = Mat4::from_cols_array(&instance.matrix);
                assert_eq!(vertex.face, expected.face_id);
                assert!(
                    matrix
                        .transform_point3(Vec3::from_array(vertex.position))
                        .distance(Vec3::from_array(expected.position))
                        < 0.001
                );
                assert!(
                    matrix
                        .transform_vector3(Vec3::from_array(vertex.normal))
                        .dot(Vec3::from_array(expected.normal))
                        > 0.9999
                );
                assert_eq!(
                    instance.color.map(|value| (value * 255.0).round() as u8)[..3],
                    expected.color[..3]
                );
            }
            for (segment, expected) in geometry.segments.iter().zip(edges.segments.iter()) {
                let matrix = Mat4::from_cols_array(&scene.instances[segment.part as usize].matrix);
                assert!(
                    matrix
                        .transform_point3(Vec3::from_array(segment.start))
                        .distance(Vec3::from_array(expected.start))
                        < 0.001
                );
                assert!(
                    matrix
                        .transform_point3(Vec3::from_array(segment.end))
                        .distance(Vec3::from_array(expected.end))
                        < 0.001
                );
            }
        }
        assert!(Geometry::new(&[]).is_err());
        Ok(())
    }
}
