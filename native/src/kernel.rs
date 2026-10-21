use cxx::UniquePtr;
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
pub enum EdgeTreatment {
    Fillet { radius_mm: f64 },
    Chamfer { width_mm: f64 },
}

#[derive(Clone, Copy, PartialEq)]
pub enum ProfileSweep {
    Extrude { height_mm: f64 },
    Revolve { angle_deg: f64 },
}
#[path = "shape_kernel.rs"]
mod shape_operations;
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
        include!("SmApiCurves.h");
        include!("SmLine.h");
        include!("SmApiBrep.h");
        include!("SmApiQueries.h");
        include!("SmFace.h");
        include!("SmSurface.h");
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
        type SmLine;
        type SmCurveArray;
        fn spice_curve_array() -> Result<UniquePtr<SmCurveArray>>;
        fn spice_copy_brep(body: &SmBrep) -> Result<UniquePtr<SmBrep>>;
        unsafe fn spice_loft(
            curves: &SmCurveArray,
            section_count: usize,
            edge_count: usize,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn spice_shell(
            body: *const SmBrep,
            thickness: f64,
            opening: *const SmFace,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiCreateSweepAlongPlanarPath(
            profile: Pin<&mut SmCurveArray>,
            path: Pin<&mut SmCurveArray>,
            center: i32,
            caps: i32,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        fn spice_profile_arc(
            center: &SmVector3d,
            radius: f64,
            start_deg: f64,
            end_deg: f64,
        ) -> Result<UniquePtr<SmCurve>>;
        fn GetMemoryUsed(self: &SmBrep, allocated: &mut u64) -> u64;
        fn IsManifoldSolid(self: &SmBrep) -> i32;
        unsafe fn spice_treat_edges(
            body: *mut SmBrep,
            edges: *const *mut SmEdge,
            count: usize,
            size_mm: f64,
            chamfer: bool,
        ) -> Result<i64>;
        unsafe fn Add(self: Pin<&mut SmCurveArray>, curve: *mut SmCurve) -> Result<u64>;
        unsafe fn SmApiCreateLineSegment(
            start: &SmVector3d,
            end: &SmVector3d,
            result: &mut *mut SmLine,
        ) -> Result<i64>;
        unsafe fn SmApiCreateLinearSweep(
            curves: &SmCurveArray,
            direction: &SmVector3d,
            distance: f64,
            cap: i32,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
        unsafe fn SmApiCreateRotationalSweep(
            curves: &SmCurveArray,
            origin: &SmVector3d,
            axis: &SmVector3d,
            angle_deg: f64,
            cap: i32,
            result: &mut *mut SmBrep,
        ) -> Result<i64>;
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
        unsafe fn SmApiFaceBoundingBox(
            face: *const SmFace,
            tight: i32,
            minimum: &mut SmVector3d,
            maximum: &mut SmVector3d,
        ) -> Result<i64>;
        unsafe fn SmApiFaceClosestPoint(
            face: *mut SmFace,
            point: &SmVector3d,
            closest: &mut SmVector3d,
            distance: &mut f64,
        ) -> Result<i64>;
        unsafe fn IsPlanar(
            self: &SmSurface,
            tolerance: f64,
            point: *mut SmVector3d,
            normal: *mut SmVector3d,
            deviation: *mut f64,
        ) -> Result<i32>;
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
        unsafe fn SmApiChamferFillet(body: *mut SmBrep, width: f64) -> Result<i64>;
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
    impl UniquePtr<SmLine> {}
}

static CONTEXT: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub struct Kernel {
    _thread: PhantomData<Rc<()>>,
    _context: std::sync::MutexGuard<'static, ()>,
}
pub struct Body<'kernel> {
    shape: UniquePtr<ffi::SmBrep>,
    volume_mm3: Cell<Option<f64>>,
    #[cfg(test)]
    volume_integrations: Cell<u32>,
    _kernel: &'kernel Kernel,
}

pub struct Tessellation {
    pub mesh: crate::mesh::Mesh,
    pub volume_mm3: f64,
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
            volume_mm3: Cell::new(None),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
            _kernel: self,
        })
    }

    pub fn sweep_profile(
        &self,
        polygon: &crate::profile::Polygon,
        sweep: ProfileSweep,
    ) -> Result<Body<'_>, String> {
        assert!((3..=crate::profile::POINT_COUNT_MAX).contains(&polygon.points.len()));
        match sweep {
            ProfileSweep::Extrude { height_mm } => {
                assert!(height_mm.is_finite());
                assert!(height_mm > 0.0);
            }
            ProfileSweep::Revolve { angle_deg } => {
                assert!(angle_deg.is_finite());
                assert!((0.1..=360.0).contains(&angle_deg));
                polygon.validate_revolve()?;
            }
        }
        let mut lines = Vec::with_capacity(polygon.points.len());
        let mut curves = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        for index in 0..polygon.points.len() {
            let start = polygon.points[index];
            let end = polygon.points[(index + 1) % polygon.points.len()];
            let point = |point: [f64; 2]| match sweep {
                ProfileSweep::Extrude { .. } => Vector {
                    x: point[0],
                    y: point[1],
                    z: 0.0,
                },
                ProfileSweep::Revolve { .. } => Vector {
                    x: point[0],
                    y: 0.0,
                    z: point[1],
                },
            };
            let start = point(start);
            let end = point(end);
            let mut result = std::ptr::null_mut();
            let status = unsafe { ffi::SmApiCreateLineSegment(&start, &end, &mut result) }
                .map_err(|error| error.to_string())?;
            let line = unsafe { UniquePtr::from_raw(result) };
            if status != 1000 || line.is_null() {
                return Err(format!("SMLib profile edge failed: {status}"));
            }
            unsafe { curves.pin_mut().Add(line.as_ptr().cast_mut().cast()) }
                .map_err(|error| error.to_string())?;
            lines.push(line);
        }
        let mut result = std::ptr::null_mut();
        let axis = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let status = unsafe {
            match sweep {
                ProfileSweep::Extrude { height_mm } => {
                    ffi::SmApiCreateLinearSweep(&curves, &axis, height_mm, 1, &mut result)
                }
                ProfileSweep::Revolve { angle_deg } => ffi::SmApiCreateRotationalSweep(
                    &curves,
                    &Vector::default(),
                    &axis,
                    angle_deg,
                    1,
                    &mut result,
                ),
            }
        }
        .map_err(|error| error.to_string())?;
        let shape = unsafe { UniquePtr::from_raw(result) };
        if status != 1000 || shape.is_null() {
            let name = match sweep {
                ProfileSweep::Extrude { .. } => "extrusion",
                ProfileSweep::Revolve { .. } => "revolve",
            };
            return Err(format!("SMLib profile {name} failed: {status}"));
        }
        Ok(Body {
            shape,
            volume_mm3: Cell::new(None),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
            _kernel: self,
        })
    }

    pub fn sweep_curved_profile(
        &self,
        profile: &crate::profile::Profile,
        sweep: ProfileSweep,
    ) -> Result<Body<'_>, String> {
        use crate::profile::Profile;
        if let Profile::Polygon(polygon) = profile {
            return self.sweep_profile(polygon, sweep);
        }
        let ProfileSweep::Extrude { height_mm } = sweep else {
            return Err("Curved and nested profiles currently support extrusion; revolve requires a polygon".into());
        };
        assert!(height_mm.is_finite());
        assert!(height_mm > 0.0);
        if let Profile::Region(loops) = profile {
            let mut ordered: Vec<_> = loops
                .iter()
                .map(|boundary| {
                    let point = boundary.segments()[0].start;
                    let depth = loops
                        .iter()
                        .filter(|other| !std::ptr::eq(*other, boundary) && other.contains(point))
                        .count();
                    (depth, boundary)
                })
                .collect();
            ordered.sort_by_key(|(depth, _)| *depth);
            let mut result: Option<Body<'_>> = None;
            for (depth, boundary) in ordered {
                let body = self.sweep_profile_loop(boundary, sweep)?;
                if let Some(result) = result.as_mut() {
                    if depth % 2 == 0 {
                        result.union(body)?;
                    } else {
                        result.subtract(body)?;
                    }
                } else {
                    assert_eq!(depth, 0);
                    result = Some(body);
                }
            }
            return result.ok_or_else(|| "Profile contains no loops".into());
        }
        self.sweep_profile_loop(profile, sweep)
    }

    fn sweep_profile_loop(
        &self,
        profile: &crate::profile::Profile,
        sweep: ProfileSweep,
    ) -> Result<Body<'_>, String> {
        if let crate::profile::Profile::Polygon(polygon) = profile {
            return self.sweep_profile(polygon, sweep);
        }
        let ProfileSweep::Extrude { height_mm } = sweep else {
            return Err("Curved profiles require extrusion".into());
        };
        let segments = profile.segments();
        let mut owners = Vec::with_capacity(segments.len());
        let mut lines = Vec::with_capacity(segments.len());
        let mut curves = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        for segment in segments {
            if let Some(center) = segment.center {
                let curve = ffi::spice_profile_arc(
                    &Vector {
                        x: center.x,
                        y: center.y,
                        z: 0.0,
                    },
                    segment.radius(),
                    segment.start_angle().to_degrees(),
                    (segment.start_angle() + segment.angle).to_degrees(),
                )
                .map_err(|error| error.to_string())?;
                if curve.is_null() {
                    return Err("SMLib profile arc creation failed".into());
                }
                unsafe { curves.pin_mut().Add(curve.as_ptr().cast_mut()) }
                    .map_err(|error| error.to_string())?;
                owners.push(curve);
            } else {
                let mut raw = std::ptr::null_mut();
                let status = unsafe {
                    ffi::SmApiCreateLineSegment(
                        &Vector {
                            x: segment.start.x,
                            y: segment.start.y,
                            z: 0.0,
                        },
                        &Vector {
                            x: segment.end.x,
                            y: segment.end.y,
                            z: 0.0,
                        },
                        &mut raw,
                    )
                }
                .map_err(|error| error.to_string())?;
                let line = unsafe { UniquePtr::from_raw(raw) };
                if status != 1000 || line.is_null() {
                    return Err(format!("SMLib profile edge failed: {status}"));
                }
                unsafe { curves.pin_mut().Add(line.as_ptr().cast_mut().cast()) }
                    .map_err(|error| error.to_string())?;
                lines.push(line);
            }
        }
        let mut raw = std::ptr::null_mut();
        let status = unsafe {
            ffi::SmApiCreateLinearSweep(
                &curves,
                &Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                height_mm,
                1,
                &mut raw,
            )
        }
        .map_err(|error| error.to_string())?;
        let shape = unsafe { UniquePtr::from_raw(raw) };
        if status != 1000 || shape.is_null() {
            return Err(format!("SMLib curved profile extrusion failed: {status}"));
        }
        Ok(Body {
            shape,
            volume_mm3: Cell::new(None),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
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
            volume_mm3: Cell::new(None),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
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
    pub fn memory_allocated_bytes(&self) -> Result<u64, String> {
        let shape = self.shape.as_ref().ok_or("Missing source body")?;
        let mut allocated = 0;
        let used = shape.GetMemoryUsed(&mut allocated);
        Ok(allocated.max(used))
    }

    pub fn try_clone(&self) -> Result<Self, String> {
        let shape = ffi::spice_copy_brep(self.shape.as_ref().ok_or("Missing source body")?)
            .map_err(|error| error.to_string())?;
        if shape.is_null() {
            return Err("SMLib body copy failed".into());
        }
        Ok(Self {
            shape,
            volume_mm3: Cell::new(self.volume_mm3.get()),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
            _kernel: self._kernel,
        })
    }

    fn boundary_edges(&self) -> Result<Vec<*mut ffi::SmEdge>, String> {
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
        Ok(edges
            .into_iter()
            .filter_map(|(edge, (_, shared))| shared.then_some(edge as *mut ffi::SmEdge))
            .collect())
    }

    pub fn edges(&self) -> Result<crate::inspection::Edges, String> {
        let mut segments = Vec::new();
        let mut items = Vec::new();
        for edge in self.boundary_edges()? {
            let edge = unsafe { &*edge };
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
        let volume_mm3 = self.volume_mm3.take();
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
        self.volume_mm3.set(volume_mm3);
        Ok(())
    }

    pub fn rotate(&mut self, axis: [f64; 3], angle: f64) -> Result<(), String> {
        let volume_mm3 = self.volume_mm3.take();
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
        self.volume_mm3.set(volume_mm3);
        Ok(())
    }

    pub fn apply_edge_treatment(
        &mut self,
        treatment: EdgeTreatment,
        references: Option<&[&crate::edge_reference::Reference]>,
    ) -> Result<(), String> {
        self.volume_mm3.set(None);
        let (name, size_mm, chamfer) = match treatment {
            EdgeTreatment::Fillet { radius_mm } => ("fillet", radius_mm, false),
            EdgeTreatment::Chamfer { width_mm } => ("chamfer", width_mm, true),
        };
        assert!(size_mm.is_finite());
        assert!(size_mm > 0.0);
        let status = if let Some(references) = references {
            assert!(!references.is_empty());
            assert!(references.len() <= crate::edge_reference::COUNT_MAX);
            let edges = self.boundary_edges()?;
            let mut selected = vec![None; references.len()];
            for edge in edges {
                let curve =
                    unsafe { (*edge).GetCurve().as_ref() }.ok_or("Missing edge treatment curve")?;
                let (mut minimum, mut maximum) = (0.0, 0.0);
                let status =
                    unsafe { ffi::SmApiEdgeParameterRange(edge, &mut minimum, &mut maximum) }
                        .map_err(|error| error.to_string())?;
                if status != 1000 || !minimum.is_finite() || !maximum.is_finite() {
                    return Err("Invalid edge treatment curve interval".into());
                }
                let mut samples = [glam::DVec3::ZERO; 17];
                samples[0] = curve_point(curve, minimum)?;
                samples[16] = curve_point(curve, maximum)?;
                if !references
                    .iter()
                    .any(|reference| reference.matches_endpoints([samples[0], samples[16]]))
                {
                    continue;
                }
                for (index, sample) in samples.iter_mut().enumerate().take(16).skip(1) {
                    *sample =
                        curve_point(curve, minimum + (maximum - minimum) * index as f64 / 16.0)?;
                }
                for (index, reference) in references.iter().enumerate() {
                    if reference.matches(&samples) {
                        if selected[index].is_some() {
                            return Err(format!(
                                "Edge '{}' is ambiguous; its endpoints match multiple edges",
                                reference.name
                            ));
                        }
                        selected[index] = Some(edge);
                    }
                }
            }
            let mut edges = Vec::with_capacity(references.len());
            for (index, edge) in selected.into_iter().enumerate() {
                let edge = edge.ok_or_else(|| format!("Edge '{}' did not match a straight edge on its source solid; update its endpoint expressions", references[index].name))?;
                if edges.contains(&edge) {
                    return Err(format!("{name} references resolve to the same edge"));
                }
                edges.push(edge);
            }
            unsafe {
                ffi::spice_treat_edges(
                    self.shape.as_mut_ptr(),
                    edges.as_ptr(),
                    edges.len(),
                    size_mm,
                    chamfer,
                )
            }
            .map_err(|error| error.to_string())?
        } else {
            let [minimum, maximum] = self.bounds(false)?;
            if size_mm >= (maximum - minimum).min_element() / 2.0 {
                return Err(format!(
                    "Whole-solid {name} size must be smaller than half the solid's smallest bounding dimension"
                ));
            }
            unsafe {
                match treatment {
                    EdgeTreatment::Fillet { radius_mm } => {
                        ffi::SmApiCircularFillet(self.shape.as_mut_ptr(), radius_mm)
                    }
                    EdgeTreatment::Chamfer { width_mm } => {
                        ffi::SmApiChamferFillet(self.shape.as_mut_ptr(), width_mm)
                    }
                }
            }
            .map_err(|error| error.to_string())?
        };
        if status != 1000 {
            return Err(format!(
                "SMLib {name} failed: {status}; reduce its size or apply it to an earlier feature"
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
        if let Some(volume) = self.volume_mm3.get() {
            return Ok(volume);
        }
        let mut volume = 0.0;
        #[cfg(test)]
        self.volume_integrations
            .set(self.volume_integrations.get() + 1);
        let status = unsafe { ffi::SmApiBrepComputeVolume(self.shape.as_ptr(), 1e-8, &mut volume) }
            .map_err(|error| error.to_string())?;
        if status != 1000 || !volume.is_finite() || volume < 0.0 {
            return Err(format!("Volume failed: {status}"));
        }
        self.volume_mm3.set(Some(volume));
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
        self.volume_mm3.set(None);
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

    pub fn planar_feature_origin(
        &mut self,
        selector: &crate::dimensions::Selector,
        offset: [f64; 2],
    ) -> Result<glam::DVec3, String> {
        assert!(selector.axis < 3);
        assert!(offset.iter().all(|value| value.is_finite()));
        let (face, bounds) = self.planar_feature_face(selector)?;
        let axes = match selector.axis {
            0 => [1, 2],
            1 => [0, 2],
            2 => [0, 1],
            _ => unreachable!(),
        };
        let mut center = (bounds[0] + bounds[1]) * 0.5;
        center[axes[0]] += offset[0];
        center[axes[1]] += offset[1];
        self.planar_feature_center(face, center)
    }

    fn planar_feature_face(
        &self,
        selector: &crate::dimensions::Selector,
    ) -> Result<(*mut ffi::SmFace, [glam::DVec3; 2]), String> {
        let mut candidates = Vec::new();
        let mut face_count = 0u32;
        for region in unsafe { topology_list(children(self.shape.as_ptr())) }? {
            let region_ref = unsafe { &*region.cast::<ffi::SmRegion>() };
            if region_ref.IsVoid() == 1 {
                continue;
            }
            for shell in unsafe { topology_list(children(region)) }? {
                for faceuse in unsafe { topology_list(children(shell)) }? {
                    face_count += 1;
                    if face_count > 10000 {
                        return Err("Face query budget exceeded".into());
                    }
                    let faceuse = unsafe { &*faceuse.cast::<ffi::SmFaceuse>() };
                    let face = faceuse.GetFace();
                    let face_ref = unsafe { face.as_ref() }.ok_or("Missing source face")?;
                    let surface =
                        unsafe { face_ref.GetSurface().as_ref() }.ok_or("Missing face surface")?;
                    let mut normal = Vector::default();
                    let planar = unsafe {
                        surface.IsPlanar(
                            1e-8,
                            std::ptr::null_mut(),
                            &mut normal,
                            std::ptr::null_mut(),
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    if planar != 1 {
                        continue;
                    }
                    let outward =
                        unsafe { faceuse.GetMate().as_ref() }.ok_or("Missing outward face side")?;
                    let mut normal = glam::DVec3::new(normal.x, normal.y, normal.z);
                    if !normal.is_finite() || normal.length_squared() < 0.5 {
                        return Err("Invalid planar face normal".into());
                    }
                    normal = normal.normalize();
                    match outward.face_orientation().0 {
                        1 => {}
                        2 => normal = -normal,
                        value => return Err(format!("Invalid outward face orientation: {value}")),
                    }
                    let expected = glam::DVec3::AXES[selector.axis]
                        * if selector.positive { 1.0 } else { -1.0 };
                    if (normal - expected).abs().max_element() > 1e-8 {
                        continue;
                    }
                    candidates.push(face);
                }
            }
        }
        let bounds = candidates.into_iter().map(|face| {
            let mut minimum = Vector::default();
            let mut maximum = Vector::default();
            let status = unsafe { ffi::SmApiFaceBoundingBox(face, 1, &mut minimum, &mut maximum) }
                .map_err(|error| error.to_string())?;
            let bounds = [
                glam::DVec3::new(minimum.x, minimum.y, minimum.z),
                glam::DVec3::new(maximum.x, maximum.y, maximum.z),
            ];
            if status != 1000
                || !bounds[0].is_finite()
                || !bounds[1].is_finite()
                || !bounds[0].cmple(bounds[1]).all()
            {
                return Err(format!("SMLib face bounds failed: {status}"));
            }
            Ok((face, bounds))
        });
        selector.resolve_candidates(bounds)
    }

    fn planar_feature_center(
        &self,
        face: *mut ffi::SmFace,
        center: glam::DVec3,
    ) -> Result<glam::DVec3, String> {
        let point = Vector {
            x: center.x,
            y: center.y,
            z: center.z,
        };
        let mut closest = Vector::default();
        let mut distance = f64::NAN;
        let status =
            unsafe { ffi::SmApiFaceClosestPoint(face, &point, &mut closest, &mut distance) }
                .map_err(|error| error.to_string())?;
        if status != 1000 || !distance.is_finite() || distance < 0.0 {
            return Err(format!("SMLib face containment failed: {status}"));
        }
        if distance > 1e-7 {
            return Err(
                "Feature center lies outside the selected face or inside an existing opening"
                    .into(),
            );
        }
        Ok(center)
    }

    pub fn mesh(&mut self) -> Result<Tessellation, String> {
        self.mesh_with_tolerance(0.02, 15.0)
    }

    pub fn game_mesh(&mut self) -> Result<Tessellation, String> {
        self.mesh_with_tolerance(0.25, 30.0)
    }

    fn mesh_with_tolerance(
        &mut self,
        chord_mm: f64,
        angle_deg: f64,
    ) -> Result<Tessellation, String> {
        assert!(chord_mm > 0.0);
        assert!(angle_deg > 0.0);
        let mut output = std::ptr::null_mut();
        let mut failed = 0;
        let mut lamina = 0;
        let status = unsafe {
            self.shape.pin_mut().ConvertToPolyBrep(
                &mut output,
                &mut failed,
                &mut lamina,
                chord_mm,
                angle_deg,
                angle_deg,
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
        let mut vertices = Vec::with_capacity((face_list.len() * 3).min(300000));
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
            let mut points = [glam::Vec3::ZERO; 4];
            let mut point_count = 0;
            for point_slot in &mut points {
                let edge_ref = unsafe { edge.as_ref() }.ok_or("Missing triangle edge")?;
                let vertex = unsafe { edge_ref.GetStartPolyVertex().as_mut() }
                    .ok_or("Missing triangle point")?;
                let point = unsafe { std::pin::Pin::new_unchecked(vertex) }.GetPoint();
                *point_slot = glam::Vec3::new(point.x as f32, point.y as f32, point.z as f32);
                point_count += 1;
                edge = unsafe { next_topology(edge) };
                if edge.is_null() || edge == edge_first {
                    break;
                }
            }
            if point_count < 3 || (!edge.is_null() && edge != edge_first) {
                return Err("Tessellation face is not a triangle or convex quad".into());
            }
            for index in 1..point_count - 1 {
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
                        color: crate::mesh::vertex_color(crate::mesh::DEFAULT_COLOR),
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
        if exact <= 0.0 {
            return Err("Tessellation requires a positive solid volume".into());
        }
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
        if (mesh.volume_mm3() - exact).abs()
            > area.mul_add(2.0 * chord_mm, exact.abs().mul_add(1e-6, 0.01))
        {
            return Err(format!(
                "Tessellation volume {} differs from exact solid {exact} ({} triangles)",
                mesh.volume_mm3(),
                mesh.vertices.len() / 3
            ));
        }
        Ok(Tessellation {
            mesh,
            volume_mm3: exact,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_cylinders_validate_at_their_requested_chord_tolerance() -> Result<(), String> {
        let (preview, game) = {
            let kernel = Kernel::new()?;
            let mut body = kernel.cylinder([0.0; 3], 5.0, 10.0)?;
            (body.mesh()?, body.game_mesh()?)
        };
        preview.mesh.validate()?;
        game.mesh.validate()?;
        assert_eq!(preview.volume_mm3, game.volume_mm3);
        assert!((game.volume_mm3 - std::f64::consts::PI * 250.0).abs() < 1e-5);
        assert!(game.mesh.vertices.len() < preview.mesh.vertices.len());
        assert!((game.mesh.volume_mm3() - game.volume_mm3).abs() > 20.0);
        Ok(())
    }

    #[test]
    fn exact_volume_reuses_queries_meshes_and_rigid_copies() -> Result<(), String> {
        let results = {
            let kernel = Kernel::new()?;
            let mut body = kernel.cylinder([0.0; 3], 5.0, 2.0)?;
            let volume = body.volume()?;
            let repeated = body.volume()?;
            let preview = body.mesh()?;
            let game = body.game_mesh()?;
            let mut copy = body.try_clone()?;
            copy.translate([17.0, -8.0, 5.0])?;
            copy.rotate([0.0, 1.0, 0.0], 37.0)?;
            let moved = copy.volume()?;
            let cached_integrations = copy.volume_integrations.get();
            copy.volume_mm3.set(None);
            let independently_integrated = copy.volume()?;
            (
                volume,
                repeated,
                preview.volume_mm3,
                game.volume_mm3,
                moved,
                independently_integrated,
                body.volume_integrations.get(),
                cached_integrations,
                copy.volume_integrations.get(),
                preview.mesh.vertices.len(),
                game.mesh.vertices.len(),
            )
        };
        let (
            volume,
            repeated,
            preview,
            game,
            moved,
            independent,
            calls,
            copy_calls,
            fresh_calls,
            preview_vertices,
            game_vertices,
        ) = results;
        assert!((volume - std::f64::consts::PI * 50.0).abs() < 1e-5);
        assert_eq!([repeated, preview, game, moved], [volume; 4]);
        assert!((independent - volume).abs() < 1e-5);
        assert_eq!(calls, 1);
        assert_eq!(copy_calls, 0);
        assert_eq!(fresh_calls, 1);
        assert!(game_vertices < preview_vertices);
        Ok(())
    }

    #[test]
    fn exact_volume_recomputes_after_shape_changes_and_step_conversion() -> Result<(), String> {
        let results = {
            let kernel = Kernel::new()?;
            let mut body = kernel.box_shape([0.0; 3], [10.0; 3])?;
            let initial = body.volume()?;
            body.subtract(kernel.box_shape([0.0; 3], [1.0, 10.0, 10.0])?)?;
            let subtract_invalidated = body.volume_mm3.get().is_none();
            let subtracted = body.volume()?;
            body.union(kernel.box_shape([9.0, 0.0, 0.0], [12.0, 10.0, 10.0])?)?;
            let union_invalidated = body.volume_mm3.get().is_none();
            let united = body.volume()?;
            body.apply_edge_treatment(EdgeTreatment::Chamfer { width_mm: 0.5 }, None)?;
            let chamfer_invalidated = body.volume_mm3.get().is_none();
            let chamfered = body.volume()?;
            let failed = body
                .apply_edge_treatment(EdgeTreatment::Fillet { radius_mm: 100.0 }, None)
                .is_err();
            let failure_invalidated = body.volume_mm3.get().is_none();
            let before_step = body.volume()?;
            body.step_data("volume-regression")?;
            let step_invalidated = body.volume_mm3.get().is_none();
            let after_step = body.volume()?;
            (
                [
                    initial,
                    subtracted,
                    united,
                    chamfered,
                    before_step,
                    after_step,
                ],
                [
                    subtract_invalidated,
                    union_invalidated,
                    chamfer_invalidated,
                    failure_invalidated,
                    step_invalidated,
                    failed,
                ],
                body.volume_integrations.get(),
            )
        };
        let (
            [
                initial,
                subtracted,
                united,
                chamfered,
                before_step,
                after_step,
            ],
            invalidated,
            calls,
        ) = results;
        assert!((initial - 1000.0).abs() < 1e-5);
        assert!((subtracted - 900.0).abs() < 1e-5);
        assert!((united - 1100.0).abs() < 1e-5);
        assert!(chamfered < united);
        assert!(chamfered > 1000.0);
        assert!((before_step - after_step).abs() < 1e-5);
        assert_eq!(invalidated, [true; 6]);
        assert_eq!(calls, 6);
        Ok(())
    }
}
