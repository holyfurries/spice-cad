use cxx::UniquePtr;
use std::marker::PhantomData;
use std::rc::Rc;
#[path = "step.rs"]
mod step;

#[repr(transparent)]
pub struct SurfaceParameter(i32);
unsafe impl cxx::ExternType for SurfaceParameter {
    type Id = cxx::type_id!("SmSurfParamType");
    type Kind = cxx::kind::Trivial;
}
#[repr(transparent)]
pub struct Orientation(i32);
unsafe impl cxx::ExternType for Orientation {
    type Id = cxx::type_id!("SmOrientType");
    type Kind = cxx::kind::Trivial;
}

#[repr(C)]
#[derive(Default)]
pub struct Vector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

unsafe impl cxx::ExternType for Vector {
    type Id = cxx::type_id!("SmVector3d");
    type Kind = cxx::kind::Opaque;
}

#[allow(clippy::too_many_arguments, reason = "Matches the native SMLib ABI")]
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("SmVector3d.h");
        include!("SmBrep.h");
        include!("SmPoly.h");
        include!("SmApiGeneral.h");
        include!("SmApiPrimitives.h");
        include!("SmApiBrep.h");
        include!("SmApiQueries.h");
        include!("SmFace.h");
        include!("SmFaceuse.h");
        include!("SmRegion.h");
        include!("SmApiFillets.h");
        include!("SmBSplineSurface.h");
        include!("SmBSplineCurve.h");
        include!("SmEdgeuse.h");
        include!("SmLoopuse.h");
        include!("SmVertexuse.h");
        include!("spice_smlib_types.h");
        type SmVector3d = super::Vector;
        type SmBrep;
        type SmPolyBrep;
        type SmPolyFace;
        type SmPolyLoop;
        type SmPolyEdge;
        type SmPolyVertex;
        type SmTopology;
        type SmOwningTopology;
        type SmFace;
        type SmFaceuse;
        type SmRegion;
        type SmObject;
        type SmSurface;
        type SmCurve;
        type SmBSplineSurface;
        type SmBSplineCurve;
        type SmLoopuse;
        type SmEdgeuse;
        type SmVertexuse;
        type SmEdge;
        type SmVertex;
        type SmSurfParamType = super::SurfaceParameter;
        type SmOrientType = super::Orientation;
        unsafe fn SmApiTurnToNurbs(body: *mut SmBrep) -> Result<i64>;
        unsafe fn SmApiVertexGetPoint(
            vertex: *const SmVertex,
            point: &mut SmVector3d,
        ) -> Result<i64>;
        unsafe fn SmApiBrepBoundingBox(
            body: *const SmBrep,
            tight: i32,
            minimum: &mut SmVector3d,
            maximum: &mut SmVector3d,
        ) -> Result<i64>;
        unsafe fn SmApiEdgeParameterRange(
            edge: *const SmEdge,
            minimum: &mut f64,
            maximum: &mut f64,
        ) -> Result<i64>;
        fn EvaluatePoint(self: &SmCurve, parameter: f64, point: &mut SmVector3d) -> Result<i64>;
        fn GetFace(self: &SmFaceuse) -> *mut SmFace;
        fn GetMate(self: &SmFaceuse) -> *mut SmFaceuse;
        #[rust_name = "face_orientation"]
        fn GetOrientation(self: &SmFaceuse) -> SmOrientType;
        fn GetSurface(self: &SmFace) -> *mut SmSurface;
        fn GetRootSurface(self: &SmSurface) -> *mut SmBSplineSurface;
        fn GetRootCurve(self: &SmCurve) -> *mut SmBSplineCurve;
        fn IsVertexLoopuse(self: &SmLoopuse) -> i32;
        fn GetVertexuse(self: &SmLoopuse) -> *mut SmVertexuse;
        fn GetVertex(self: &SmVertexuse) -> *mut SmVertex;
        fn GetEUorVU(self: &SmLoopuse) -> *mut SmTopology;
        fn GetCCWEdgeuse(self: &SmEdgeuse) -> *mut SmEdgeuse;
        fn GetEdge(self: &SmEdgeuse) -> *mut SmEdge;
        #[rust_name = "edge_orientation"]
        fn GetOrientation(self: &SmEdgeuse) -> SmOrientType;
        fn GetStartVertex(self: &SmEdge) -> *mut SmVertex;
        fn GetEndVertex(self: &SmEdge) -> *mut SmVertex;
        fn GetCurve(self: &SmEdge) -> *mut SmCurve;
        #[rust_name = "surface_degree"]
        fn GetDegree(self: &SmBSplineSurface, parameter: SmSurfParamType) -> u64;
        #[rust_name = "curve_degree"]
        fn GetDegree(self: &SmBSplineCurve) -> u64;
        fn GetNumberNaturalKnots(self: &SmBSplineCurve) -> u64;
        unsafe fn GetKnotsExpert(
            self: &SmBSplineCurve,
            first: u64,
            last: u64,
            knots: *mut f64,
        ) -> Result<i64>;
        unsafe fn GetKnotsPointers(
            self: &SmBSplineSurface,
            u_count: &mut u64,
            v_count: &mut u64,
            u_knots: &mut *mut f64,
            v_knots: &mut *mut f64,
        ) -> Result<i64>;
        #[rust_name = "surface_points"]
        unsafe fn GetControlPointsPointer(
            self: &SmBSplineSurface,
            u_count: &mut u64,
            v_count: &mut u64,
            points: &mut *mut f64,
        ) -> Result<i64>;
        #[rust_name = "curve_points"]
        unsafe fn GetControlPointsPointer(
            self: &SmBSplineCurve,
            count: &mut u64,
            points: &mut *mut f64,
        ) -> Result<i64>;
        #[rust_name = "surface_rational"]
        fn IsRational(self: &SmBSplineSurface) -> i32;
        #[rust_name = "curve_rational"]
        fn IsRational(self: &SmBSplineCurve) -> i32;
        #[cxx_name = "SmFailureArray"]
        type Failures;
        type SmTessellationReport;
        fn SmApiCreateContext() -> Result<()>;
        unsafe fn SmApiCreateBox(
            origin: &SmVector3d,
            x: f64,
            y: f64,
            z: f64,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiCreateCylinder(
            origin: &SmVector3d,
            radius: f64,
            height: f64,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiTranslate(object: *mut SmObject, offset: &SmVector3d) -> Result<i64>;
        unsafe fn SmApiRotate(
            object: *mut SmObject,
            origin: &SmVector3d,
            axis: &SmVector3d,
            angle: f64,
        ) -> Result<i64>;
        unsafe fn SmApiCircularFillet(body: *mut SmBrep, radius: f64) -> Result<i64>;
        unsafe fn SmApiBooleanUnion(
            first: *mut SmBrep,
            second: *mut SmBrep,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiBooleanDifference(
            first: *mut SmBrep,
            second: *mut SmBrep,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiBrepComputeVolume(
            body: *const SmBrep,
            accuracy: f64,
            result: &mut f64,
        ) -> Result<i64>;
        unsafe fn ConvertToPolyBrep(
            self: Pin<&mut SmBrep>,
            result: &mut *mut SmPolyBrep,
            failed: &mut i32,
            lamina: &mut u64,
            chord: f64,
            curve: f64,
            surface: f64,
            edge: f64,
            aspect: f64,
            advancing: i32,
            smooth: i32,
            attributes: i32,
            context: i32,
            failures: *mut Failures,
            report: *mut SmTessellationReport,
        ) -> Result<i64>;
        fn GetList(self: &SmOwningTopology) -> *mut SmTopology;
        fn GetOuterPolyLoop(self: &SmPolyFace) -> *mut SmPolyLoop;
        fn GetOriginalFace(self: &SmPolyFace) -> *mut SmFace;
        fn GetUpwardFaceuse(self: &SmFace) -> *mut SmFaceuse;
        fn GetRegion(self: &SmFaceuse) -> *mut SmRegion;
        fn IsVoid(self: &SmRegion) -> i32;
        fn IsInfiniteRegion(self: &SmRegion) -> i32;
        fn GetFirstPolyEdge(self: &SmPolyLoop) -> *mut SmPolyEdge;
        fn GetNext(self: &SmTopology) -> *mut SmTopology;
        fn GetStartPolyVertex(self: &SmPolyEdge) -> *mut SmPolyVertex;
        fn GetPoint(self: Pin<&mut SmPolyVertex>) -> &mut SmVector3d;
    }
    impl UniquePtr<SmBrep> {}
    impl UniquePtr<SmPolyBrep> {}
}

static CONTEXT: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub struct Kernel {
    _thread: PhantomData<Rc<()>>,
    _context: std::sync::MutexGuard<'static, ()>,
}
pub struct Body<'kernel> {
    shape: UniquePtr<ffi::SmBrep>,
    _kernel: &'kernel Kernel,
}

impl Kernel {
    pub fn new() -> Result<Self, String> {
        let context = CONTEXT.lock().map_err(|error| error.to_string())?;
        ffi::SmApiCreateContext().map_err(|error| error.to_string())?;
        Ok(Self {
            _thread: PhantomData,
            _context: context,
        })
    }

    pub fn box_shape(&self, minimum: [f64; 3], maximum: [f64; 3]) -> Result<Body<'_>, String> {
        assert!(minimum.iter().all(|value| value.is_finite()));
        assert!(maximum.iter().all(|value| value.is_finite()));
        let size: [f64; 3] = std::array::from_fn(|index| maximum[index] - minimum[index]);
        if size.iter().any(|value| *value <= 0.0) {
            return Err("Box dimensions must be positive".into());
        }
        let origin = Vector {
            x: minimum[0],
            y: minimum[1],
            z: minimum[2],
        };
        let mut result = std::ptr::null_mut();
        let status =
            unsafe { ffi::SmApiCreateBox(&origin, size[0], size[1], size[2], &mut result) }
                .map_err(|error| error.to_string())?;
        let shape = unsafe { UniquePtr::from_raw(result) };
        if status != 1000 || shape.is_null() {
            return Err(format!("SMLib box failed: {status}"));
        }
        Ok(Body {
            shape,
            _kernel: self,
        })
    }

    pub fn cylinder(&self, origin: [f64; 3], radius: f64, height: f64) -> Result<Body<'_>, String> {
        assert!(radius.is_finite() && radius > 0.0);
        assert!(height.is_finite() && height > 0.0);
        let origin = Vector {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        };
        let mut result = std::ptr::null_mut();
        let status = unsafe { ffi::SmApiCreateCylinder(&origin, radius, height, &mut result) }
            .map_err(|error| error.to_string())?;
        let shape = unsafe { UniquePtr::from_raw(result) };
        if status != 1000 || shape.is_null() {
            return Err(format!("SMLib cylinder failed: {status}"));
        }
        Ok(Body {
            shape,
            _kernel: self,
        })
    }

    pub fn rack(&self, parameters: spice_rack::RackParameters) -> Result<Body<'_>, String> {
        let layout = parameters.layout()?;
        let width = layout.dimensions_mm.x;
        let depth = layout.dimensions_mm.y;
        let thickness = parameters.thickness_mm;
        let mut rack = self.box_shape([0.0; 3], [width, depth, thickness])?;
        for index in 0..parameters.step_count {
            let y = f64::from(index) * layout.tread_depth_mm;
            let z = f64::from(index) * parameters.step_rise_mm;
            let height = z + thickness + parameters.lip_height_mm;
            for x in [0.0, width - thickness] {
                rack.union(self.box_shape(
                    [x, y, 0.0],
                    [x + thickness, y + layout.tread_depth_mm, height],
                )?)?;
            }
            rack.union(self.box_shape(
                [0.0, y, z],
                [width, y + layout.tread_depth_mm, z + thickness],
            )?)?;
            rack.union(self.box_shape(
                [0.0, y, z - parameters.step_rise_mm.min(z)],
                [width, y + thickness, height],
            )?)?;
        }
        rack.union(self.box_shape(
            [0.0, depth - thickness, 0.0],
            layout.dimensions_mm.to_array(),
        )?)?;
        Ok(rack)
    }
}

impl Body<'_> {
    pub fn edges(&self) -> Result<crate::inspection::Edges, String> {
        let mut edges = std::collections::BTreeMap::new();
        for region in unsafe { topology_list(children(self.shape.as_ptr())) }? {
            let region_ref = unsafe { &*region.cast::<ffi::SmRegion>() };
            if region_ref.IsVoid() == 1 {
                continue;
            }
            for shell in unsafe { topology_list(children(region)) }? {
                for faceuse in unsafe { topology_list(children(shell)) }? {
                    let face = unsafe { &*faceuse.cast::<ffi::SmFaceuse>() }.GetFace() as usize;
                    for outline in unsafe { topology_list(children(faceuse)) }? {
                        let outline = unsafe { &*outline.cast::<ffi::SmLoopuse>() };
                        if outline.IsVertexLoopuse() == 1 {
                            continue;
                        }
                        let first = outline.GetEUorVU().cast::<ffi::SmEdgeuse>();
                        let mut current = first;
                        for _ in 0..10000 {
                            let edgeuse =
                                unsafe { current.as_ref() }.ok_or("Missing display edge")?;
                            let entry = edges
                                .entry(edgeuse.GetEdge() as usize)
                                .or_insert((face, false));
                            if entry.0 != face {
                                entry.1 = true;
                            }
                            if edges.len() > 10000 {
                                return Err("Display edge budget exceeded".into());
                            }
                            current = edgeuse.GetCCWEdgeuse();
                            if current == first {
                                break;
                            }
                        }
                        if current != first {
                            return Err("Display edge loop exceeds its budget".into());
                        }
                    }
                }
            }
        }
        let mut segments = Vec::new();
        let mut items = Vec::new();
        for (edge, (_, shared)) in edges {
            if !shared {
                continue;
            }
            let edge = unsafe { &*(edge as *const ffi::SmEdge) };
            let curve = unsafe { edge.GetCurve().as_ref() }.ok_or("Missing display curve")?;
            let (mut minimum, mut maximum) = (0.0, 0.0);
            let status = unsafe { ffi::SmApiEdgeParameterRange(edge, &mut minimum, &mut maximum) }
                .map_err(|error| error.to_string())?;
            if status != 1000 || !minimum.is_finite() || !maximum.is_finite() {
                return Err("Invalid display curve interval".into());
            }
            let start = curve_point(curve, minimum)?;
            let end = curve_point(curve, maximum)?;
            let mut samples = [glam::DVec3::ZERO; 17];
            for (index, sample) in samples.iter_mut().enumerate() {
                *sample = curve_point(curve, minimum + (maximum - minimum) * index as f64 / 16.0)?;
            }
            let segment_start = segments.len() as u32;
            let mut pending = vec![(minimum, start, maximum, end, 0_u32)];
            for _ in 0..4096 {
                let Some((minimum, start, maximum, end, depth)) = pending.pop() else {
                    break;
                };
                let mut error_mm = 0.0_f64;
                let mut middle = glam::DVec3::ZERO;
                for fraction in [0.25, 0.5, 0.75] {
                    let point = curve_point(curve, minimum + (maximum - minimum) * fraction)?;
                    let direction = end - start;
                    let along = if direction.length_squared() > 1e-20 {
                        ((point - start).dot(direction) / direction.length_squared())
                            .clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    error_mm = error_mm.max(point.distance(start + direction * along));
                    if fraction == 0.5 {
                        middle = point;
                    }
                }
                if error_mm > 0.02 {
                    if depth >= 16 {
                        return Err("Display curve cannot meet its chord tolerance".into());
                    }
                    let parameter = (minimum + maximum) / 2.0;
                    pending.extend([
                        (parameter, middle, maximum, end, depth + 1),
                        (minimum, start, parameter, middle, depth + 1),
                    ]);
                } else if start.distance_squared(end) > 1e-12 {
                    if segments.len() >= 131072 {
                        return Err("Display segment budget exceeded".into());
                    }
                    segments.push(crate::mesh::EdgeSegment {
                        start: start.as_vec3().to_array(),
                        end: end.as_vec3().to_array(),
                    });
                }
            }
            if !pending.is_empty() {
                return Err("Display curve sampling budget exceeded".into());
            }
            if segments.len() as u32 > segment_start {
                items.push(crate::inspection::Edge::from_samples(
                    [segment_start, segments.len() as u32],
                    samples,
                ));
            }
        }
        Ok(crate::inspection::Edges { segments, items })
    }
    pub fn translate(&mut self, offset: [f64; 3]) -> Result<(), String> {
        let offset = Vector {
            x: offset[0],
            y: offset[1],
            z: offset[2],
        };
        let status = unsafe { ffi::SmApiTranslate(self.shape.as_mut_ptr().cast(), &offset) }
            .map_err(|error| error.to_string())?;
        if status != 1000 {
            return Err(format!("SMLib translation failed: {status}"));
        }
        Ok(())
    }

    pub fn rotate(&mut self, axis: [f64; 3], angle: f64) -> Result<(), String> {
        let axis = Vector {
            x: axis[0],
            y: axis[1],
            z: axis[2],
        };
        let status = unsafe {
            ffi::SmApiRotate(
                self.shape.as_mut_ptr().cast(),
                &Vector::default(),
                &axis,
                angle,
            )
        }
        .map_err(|error| error.to_string())?;
        if status != 1000 {
            return Err(format!("SMLib rotation failed: {status}"));
        }
        Ok(())
    }

    pub fn fillet(&mut self, radius: f64) -> Result<(), String> {
        let [minimum, maximum] = self.bounds(false)?;
        if radius >= (maximum - minimum).min_element() / 2.0 {
            return Err(
                "Fillet radius must be smaller than half the solid's smallest bounding dimension"
                    .into(),
            );
        }
        let status = unsafe { ffi::SmApiCircularFillet(self.shape.as_mut_ptr(), radius) }
            .map_err(|error| error.to_string())?;
        if status != 1000 {
            return Err(format!(
                "SMLib fillet failed: {status}; reduce the radius or fillet an earlier feature"
            ));
        }
        Ok(())
    }

    pub fn bounds(&self, tight: bool) -> Result<[glam::DVec3; 2], String> {
        let mut minimum = Vector::default();
        let mut maximum = Vector::default();
        let status = unsafe {
            ffi::SmApiBrepBoundingBox(
                self.shape.as_ptr(),
                i32::from(tight),
                &mut minimum,
                &mut maximum,
            )
        }
        .map_err(|error| error.to_string())?;
        if status != 1000 {
            return Err(format!("SMLib bounds query failed: {status}"));
        }
        let minimum = glam::dvec3(minimum.x, minimum.y, minimum.z);
        let maximum = glam::dvec3(maximum.x, maximum.y, maximum.z);
        if !minimum.is_finite() || !maximum.is_finite() || maximum.cmplt(minimum).any() {
            return Err("SMLib returned invalid bounds".into());
        }
        Ok([minimum, maximum])
    }
    pub fn volume(&self) -> Result<f64, String> {
        let mut volume = 0.0;
        let status = unsafe { ffi::SmApiBrepComputeVolume(self.shape.as_ptr(), 1e-8, &mut volume) }
            .map_err(|error| error.to_string())?;
        if status != 1000 {
            return Err(format!("Volume failed: {status}"));
        }
        Ok(volume)
    }
    pub fn union(&mut self, other: Body) -> Result<(), String> {
        self.boolean(other, false)
    }

    pub fn subtract(&mut self, other: Body) -> Result<(), String> {
        self.boolean(other, true)
    }

    fn boolean(&mut self, other: Body, subtract: bool) -> Result<(), String> {
        assert!(!self.shape.is_null());
        assert!(!other.shape.is_null());
        let first = self.shape.as_mut_ptr();
        let second = other.shape.into_raw();
        let mut result = std::ptr::null_mut();
        let operation = unsafe {
            if subtract {
                ffi::SmApiBooleanDifference(first, second, &mut result)
            } else {
                ffi::SmApiBooleanUnion(first, second, &mut result)
            }
        };
        match operation {
            Ok(1000) => {
                assert_eq!(result, first);
                Ok(())
            }
            other => {
                drop(unsafe { UniquePtr::from_raw(second) });
                Err(format!(
                    "SMLib boolean failed (subtract={subtract}): {other:?}"
                ))
            }
        }
    }

    pub fn mesh(&mut self) -> Result<crate::mesh::Mesh, String> {
        let mut output = std::ptr::null_mut();
        let mut failed = 0;
        let mut lamina = 0;
        let status = unsafe {
            self.shape.pin_mut().ConvertToPolyBrep(
                &mut output,
                &mut failed,
                &mut lamina,
                0.02,
                15.0,
                15.0,
                0.0,
                0.0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
        .map_err(|error| error.to_string())?;
        let poly = unsafe { UniquePtr::from_raw(output) };
        if status != 1000 || failed != 0 || lamina != 0 || poly.is_null() {
            return Err(format!(
                "SMLib tessellation failed: status {status}, failed {failed}, open edges {lamina}"
            ));
        }
        let mut face_list = Vec::new();
        let regions = unsafe { topology_list(children(poly.as_ptr())) }?;
        for region in regions {
            for shell in unsafe { topology_list(children(region)) }? {
                let faces = unsafe { topology_list(children(shell)) }?;
                if face_list.len() + faces.len() > 100000 {
                    return Err("Tessellation face budget exceeded".into());
                }
                face_list.extend(faces);
            }
        }
        let mut vertices = Vec::with_capacity(3072);
        let mut faces = std::collections::BTreeMap::new();
        for face in face_list {
            let face = face.cast::<ffi::SmPolyFace>();
            let face_ref = unsafe { face.as_ref() }.ok_or("Empty tessellation face")?;
            let origin = face_ref.GetOriginalFace() as usize;
            let original =
                unsafe { face_ref.GetOriginalFace().as_ref() }.ok_or("Missing source face")?;
            let faceuse = unsafe { original.GetUpwardFaceuse().as_ref() }
                .ok_or("Missing source face side")?;
            let region = unsafe { faceuse.GetRegion().as_ref() }.ok_or("Missing source region")?;
            let reverse = region.IsVoid() == 1 && region.IsInfiniteRegion() != 1;
            let next_id = u32::try_from(faces.len() + 1).map_err(|error| error.to_string())?;
            let face_id = *faces.entry(origin).or_insert(next_id);
            let outline =
                unsafe { face_ref.GetOuterPolyLoop().as_ref() }.ok_or("Missing face outline")?;
            let edge_first = outline.GetFirstPolyEdge();
            let mut edge = edge_first;
            let mut points = Vec::with_capacity(4);
            for _ in 0..16 {
                let edge_ref = unsafe { edge.as_ref() }.ok_or("Missing triangle edge")?;
                let vertex = unsafe { edge_ref.GetStartPolyVertex().as_mut() }
                    .ok_or("Missing triangle point")?;
                let point = unsafe { std::pin::Pin::new_unchecked(vertex) }.GetPoint();
                points.push(glam::Vec3::new(
                    point.x as f32,
                    point.y as f32,
                    point.z as f32,
                ));
                edge = unsafe { next_topology(edge) };
                if edge.is_null() || edge == edge_first {
                    break;
                }
            }
            if !(3..=4).contains(&points.len()) {
                return Err("Tessellation face is not a triangle or convex quad".into());
            }
            for index in 1..points.len() - 1 {
                let triangle = if reverse {
                    [points[0], points[index + 1], points[index]]
                } else {
                    [points[0], points[index], points[index + 1]]
                };
                let normal = (triangle[1] - triangle[0])
                    .cross(triangle[2] - triangle[0])
                    .normalize_or_zero();
                if !normal.is_finite() || normal.length_squared() < 0.5 {
                    return Err("Degenerate tessellation triangle".into());
                }
                if vertices.len() + 3 > 300000 {
                    return Err("Tessellation vertex budget exceeded".into());
                }
                for point in triangle {
                    vertices.push(crate::mesh::Vertex {
                        position: point.to_array(),
                        normal: normal.to_array(),
                        face_id,
                    });
                }
            }
        }
        let mesh = crate::mesh::Mesh {
            vertices,
            face_count: faces.len() as u32,
        };
        mesh.validate()?;
        let exact = self.volume()?;
        let area: f64 = mesh
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let points =
                    triangle.map(|vertex| glam::DVec3::from_array(vertex.position.map(f64::from)));
                (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .length()
                    / 2.0
            })
            .sum();
        if (mesh.volume_mm3() - exact).abs() > area.mul_add(0.04, exact.abs().mul_add(1e-6, 0.01)) {
            return Err(format!(
                "Tessellation volume {} differs from exact solid {exact} ({} triangles)",
                mesh.volume_mm3(),
                mesh.vertices.len() / 3
            ));
        }
        Ok(mesh)
    }
}

fn curve_point(curve: &ffi::SmCurve, parameter: f64) -> Result<glam::DVec3, String> {
    let mut point = Vector::default();
    let status = curve
        .EvaluatePoint(parameter, &mut point)
        .map_err(|error| error.to_string())?;
    let point = glam::dvec3(point.x, point.y, point.z);
    if status != 1000 || !point.is_finite() {
        return Err("Cannot evaluate display curve".into());
    }
    Ok(point)
}

// The pinned kernel uses a single inheritance chain for its topology classes.
unsafe fn next_topology<T>(value: *mut T) -> *mut T {
    assert!(!value.is_null());
    unsafe { (&*value.cast::<ffi::SmTopology>()).GetNext().cast() }
}

unsafe fn children<T>(value: *const T) -> *mut ffi::SmTopology {
    assert!(!value.is_null());
    unsafe { (&*value.cast::<ffi::SmOwningTopology>()).GetList() }
}

unsafe fn topology_list(first: *mut ffi::SmTopology) -> Result<Vec<*mut ffi::SmTopology>, String> {
    let mut values = Vec::new();
    if first.is_null() {
        return Ok(values);
    }
    let mut current = first;
    for _ in 0..100000 {
        values.push(current);
        current = unsafe { next_topology(current) };
        if current.is_null() || current == first {
            return Ok(values);
        }
    }
    Err("Topology list exceeds its budget".into())
}
