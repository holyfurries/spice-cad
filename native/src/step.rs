use super::{Body, SurfaceParameter, Vector, children, ffi, topology_list};
use std::collections::BTreeMap;

const ENTITY_COUNT_MAX: usize = 200000;
const FILE_SIZE_BYTES_MAX: usize = 32 * 1024 * 1024;
const CONTROL_POINT_COUNT_MAX: u64 = 65536;

struct Writer {
    entities: Vec<String>,
    size_bytes: usize,
    vertices: BTreeMap<usize, u32>,
    edges: BTreeMap<usize, u32>,
}

impl Body<'_> {
    pub fn step_data(&mut self, name: &str) -> Result<String, String> {
        self.volume_mm3.set(None);
        let status = unsafe { ffi::SmApiTurnToNurbs(self.shape.as_mut_ptr()) }
            .map_err(|error| error.to_string())?;
        status_ok(status)?;
        let mut writer = Writer {
            entities: Vec::new(),
            size_bytes: 0,
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
        };
        let mut solids = Vec::new();
        for region in unsafe { topology_list(children(self.shape.as_ptr())) }? {
            let region = unsafe { &*region.cast::<ffi::SmRegion>() };
            if region.IsVoid() == 1 || region.IsInfiniteRegion() == 1 {
                continue;
            }
            let mut shells = Vec::new();
            for shell in unsafe { topology_list(children(region)) }? {
                let mut faces = Vec::new();
                for faceuse in unsafe { topology_list(children(shell)) }? {
                    let faceuse = unsafe { &*faceuse.cast::<ffi::SmFaceuse>() };
                    let outside = unsafe { faceuse.GetMate().as_ref() }
                        .ok_or("STEP: missing outward face side")?;
                    faces.push(writer.face(outside)?);
                }
                if faces.is_empty() {
                    return Err("STEP: empty shell".into());
                }
                shells.push(writer.add(format!("CLOSED_SHELL('',({}))", references(&faces)))?);
            }
            let Some(&outer) = shells.first() else {
                return Err("STEP: empty solid".into());
            };
            let name = step_string(name);
            let solid = if shells.len() == 1 {
                writer.add(format!("MANIFOLD_SOLID_BREP({name},#{outer})"))?
            } else {
                let mut voids = Vec::new();
                for shell in &shells[1..] {
                    voids.push(writer.add(format!("ORIENTED_CLOSED_SHELL('',*,#{shell},.F.)"))?);
                }
                writer.add(format!(
                    "BREP_WITH_VOIDS({name},#{outer},({}))",
                    references(&voids)
                ))?
            };
            solids.push(solid);
        }
        if solids.is_empty() {
            return Err("STEP: model contains no solids".into());
        }
        writer.finish(name, &solids)
    }
}

impl Writer {
    fn add(&mut self, entity: String) -> Result<u32, String> {
        if self.entities.len() >= ENTITY_COUNT_MAX {
            return Err("STEP entity budget exceeded".into());
        }
        self.size_bytes += entity.len() + 16;
        if self.size_bytes > FILE_SIZE_BYTES_MAX {
            return Err("STEP file exceeds 32 MiB".into());
        }
        self.entities.push(entity);
        Ok(self.entities.len() as u32)
    }

    fn point(&mut self, point: [f64; 3]) -> Result<u32, String> {
        self.add(format!(
            "CARTESIAN_POINT('',({},{},{}))",
            real(point[0])?,
            real(point[1])?,
            real(point[2])?
        ))
    }

    fn vertex(&mut self, vertex: *mut ffi::SmVertex) -> Result<u32, String> {
        if vertex.is_null() {
            return Err("STEP: missing vertex".into());
        }
        if let Some(id) = self.vertices.get(&(vertex as usize)) {
            return Ok(*id);
        }
        let mut point = Vector::default();
        status_ok(
            unsafe { ffi::SmApiVertexGetPoint(vertex, &mut point) }
                .map_err(|error| error.to_string())?,
        )?;
        let point = self.point([point.x, point.y, point.z])?;
        let id = self.add(format!("VERTEX_POINT('',#{point})"))?;
        self.vertices.insert(vertex as usize, id);
        Ok(id)
    }

    fn poles(
        &mut self,
        pointer: *mut f64,
        count: u64,
        rational: bool,
    ) -> Result<(Vec<u32>, Vec<String>), String> {
        if count == 0 || count > CONTROL_POINT_COUNT_MAX {
            return Err("STEP control point budget exceeded".into());
        }
        let points = unsafe { pointer_values(pointer, count * 4) }?;
        let mut ids = Vec::with_capacity(count as usize);
        let mut weights = Vec::with_capacity(count as usize);
        for point in points.as_chunks::<4>().0 {
            let weight = if rational { point[3] } else { 1.0 };
            if !weight.is_finite() || weight <= 0.0 {
                return Err("STEP: invalid rational weight".into());
            }
            ids.push(self.point([point[0] / weight, point[1] / weight, point[2] / weight])?);
            weights.push(real(weight)?);
        }
        Ok((ids, weights))
    }

    fn edge(&mut self, edge: *mut ffi::SmEdge) -> Result<u32, String> {
        if edge.is_null() {
            return Err("STEP: missing edge".into());
        }
        if let Some(id) = self.edges.get(&(edge as usize)) {
            return Ok(*id);
        }
        let edge_ref = unsafe { &*edge };
        let curve = unsafe { edge_ref.GetCurve().as_ref() }.ok_or("STEP: missing curve")?;
        let curve =
            unsafe { curve.GetRootCurve().as_ref() }.ok_or("STEP: unsupported non-NURBS curve")?;
        let count = curve.GetNumberNaturalKnots();
        if !(2..=65536).contains(&count) {
            return Err("STEP knot budget exceeded".into());
        }
        let mut knots = vec![0.0; count as usize];
        status_ok(
            unsafe { curve.GetKnotsExpert(0, count - 1, knots.as_mut_ptr()) }
                .map_err(|error| error.to_string())?,
        )?;
        let mut count = 0;
        let mut pointer = std::ptr::null_mut();
        status_ok(
            unsafe { curve.curve_points(&mut count, &mut pointer) }
                .map_err(|error| error.to_string())?,
        )?;
        let (poles, weights) = self.poles(pointer, count, curve.curve_rational() == 1)?;
        let degree = curve.curve_degree();
        validate_spline(degree, count, knots.len())?;
        let (multiplicities, knots) = knot_data(&knots)?;
        let poles = references(&poles);
        let curve = self.add(format!("(BOUNDED_CURVE() B_SPLINE_CURVE({degree},({poles}),.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS(({multiplicities}),({knots}),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))", weights.join(",")))?;
        let first = self.vertex(edge_ref.GetStartVertex())?;
        let last = self.vertex(edge_ref.GetEndVertex())?;
        let id = self.add(format!("EDGE_CURVE('',#{first},#{last},#{curve},.T.)"))?;
        self.edges.insert(edge as usize, id);
        Ok(id)
    }

    fn surface(&mut self, surface: &ffi::SmSurface) -> Result<u32, String> {
        let surface = unsafe { surface.GetRootSurface().as_ref() }
            .ok_or("STEP: unsupported non-NURBS surface")?;
        let (mut u_count, mut v_count) = (0, 0);
        let mut pointer = std::ptr::null_mut();
        status_ok(
            unsafe { surface.surface_points(&mut u_count, &mut v_count, &mut pointer) }
                .map_err(|error| error.to_string())?,
        )?;
        if u_count == 0 || v_count == 0 || u_count > CONTROL_POINT_COUNT_MAX / v_count {
            return Err("STEP surface control point budget exceeded".into());
        }
        let (poles, weights) =
            self.poles(pointer, u_count * v_count, surface.surface_rational() == 1)?;
        let (mut u_size, mut v_size) = (0, 0);
        let (mut u_pointer, mut v_pointer) = (std::ptr::null_mut(), std::ptr::null_mut());
        status_ok(
            unsafe {
                surface.GetKnotsPointers(&mut u_size, &mut v_size, &mut u_pointer, &mut v_pointer)
            }
            .map_err(|error| error.to_string())?,
        )?;
        let u_knots = unsafe { pointer_values(u_pointer, u_size) }?;
        let v_knots = unsafe { pointer_values(v_pointer, v_size) }?;
        let u_degree = surface.surface_degree(SurfaceParameter(0));
        let v_degree = surface.surface_degree(SurfaceParameter(1));
        validate_spline(u_degree, u_count, u_knots.len())?;
        validate_spline(v_degree, v_count, v_knots.len())?;
        let (u_multiplicities, u_knots) = knot_data(&u_knots)?;
        let (v_multiplicities, v_knots) = knot_data(&v_knots)?;
        let net = poles
            .chunks(v_count as usize)
            .map(|row| format!("({})", references(row)))
            .collect::<Vec<_>>()
            .join(",");
        let weights = weights
            .chunks(v_count as usize)
            .map(|row| format!("({})", row.join(",")))
            .collect::<Vec<_>>()
            .join(",");
        self.add(format!("(BOUNDED_SURFACE() B_SPLINE_SURFACE({u_degree},{v_degree},({net}),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS(({u_multiplicities}),({v_multiplicities}),({u_knots}),({v_knots}),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({weights})) REPRESENTATION_ITEM('') SURFACE())"))
    }

    fn face(&mut self, faceuse: &ffi::SmFaceuse) -> Result<u32, String> {
        let face = unsafe { faceuse.GetFace().as_ref() }.ok_or("STEP: missing face")?;
        let surface = unsafe { face.GetSurface().as_ref() }.ok_or("STEP: missing surface")?;
        let surface = self.surface(surface)?;
        let mut bounds = Vec::new();
        for outline in unsafe { topology_list(children(faceuse)) }? {
            let outline = unsafe { &*outline.cast::<ffi::SmLoopuse>() };
            let path = if outline.IsVertexLoopuse() == 1 {
                let vertexuse = unsafe { outline.GetVertexuse().as_ref() }
                    .ok_or("STEP: missing pole vertex")?;
                let vertex = self.vertex(vertexuse.GetVertex())?;
                self.add(format!("VERTEX_LOOP('',#{vertex})"))?
            } else {
                let first = outline.GetEUorVU().cast::<ffi::SmEdgeuse>();
                let mut current = first;
                let mut edges = Vec::new();
                for _ in 0..10000 {
                    let edgeuse = unsafe { current.as_ref() }.ok_or("STEP: missing loop edge")?;
                    let edge = self.edge(edgeuse.GetEdge())?;
                    let orientation = flag(edgeuse.edge_orientation().0 == 1);
                    edges.push(self.add(format!("ORIENTED_EDGE('',*,*,#{edge},{orientation})"))?);
                    current = edgeuse.GetCCWEdgeuse();
                    if current == first {
                        break;
                    }
                }
                if current != first || edges.is_empty() {
                    return Err("STEP: loop is open or exceeds its budget".into());
                }
                self.add(format!("EDGE_LOOP('',({}))", references(&edges)))?
            };
            let kind = if bounds.is_empty() {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(self.add(format!("{kind}('',#{path},.T.)"))?);
        }
        if bounds.is_empty() {
            return Err("STEP: face has no bounds".into());
        }
        self.add(format!(
            "ADVANCED_FACE('',({}),#{surface},{})",
            references(&bounds),
            flag(faceuse.face_orientation().0 == 1)
        ))
    }

    fn finish(&mut self, name: &str, solids: &[u32]) -> Result<String, String> {
        let name = step_string(name);
        let application = self.add(
            "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into(),
        )?;
        self.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{application})"))?;
        let context = self.add(format!("PRODUCT_CONTEXT('',#{application},'mechanical')"))?;
        let product = self.add(format!("PRODUCT({name},{name},'',(#{context}))"))?;
        let formation = self.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"))?;
        let context = self.add(format!(
            "PRODUCT_DEFINITION_CONTEXT('part definition',#{application},'design')"
        ))?;
        let definition = self.add(format!(
            "PRODUCT_DEFINITION('design','',#{formation},#{context})"
        ))?;
        let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"))?;
        let length = self.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into())?;
        let angle = self.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".into())?;
        let solid_angle =
            self.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())".into())?;
        let uncertainty = self.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{length},'distance_accuracy_value','')"))?;
        let context = self.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle})) REPRESENTATION_CONTEXT('',''))"))?;
        let representation = self.add(format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION({name},({}),#{context})",
            references(solids)
        ))?;
        self.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
        ))?;
        let mut output = String::from(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Native SMLib model'),'2;1');\nFILE_NAME('model.step','',(''),(''),'spice-cad','','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n",
        );
        for (index, entity) in self.entities.iter().enumerate() {
            use std::fmt::Write;
            writeln!(output, "#{}={entity};", index + 1).map_err(|error| error.to_string())?;
        }
        output.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        assert!(output.len() <= FILE_SIZE_BYTES_MAX + 1024);
        Ok(output)
    }
}

unsafe fn pointer_values(pointer: *const f64, count: u64) -> Result<Vec<f64>, String> {
    if pointer.is_null() || count == 0 || count > CONTROL_POINT_COUNT_MAX * 4 {
        return Err("STEP array is missing or exceeds its budget".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, count as usize) }.to_vec())
}

fn validate_spline(degree: u64, count: u64, knot_count: usize) -> Result<(), String> {
    if !(1..=32).contains(&degree) || count <= degree || knot_count as u64 != count + degree + 1 {
        return Err("STEP: inconsistent NURBS degree, knots, or control points".into());
    }
    Ok(())
}

fn knot_data(knots: &[f64]) -> Result<(String, String), String> {
    let mut values = Vec::new();
    let mut multiplicities = Vec::<u32>::new();
    let mut last = f64::NEG_INFINITY;
    for &value in knots {
        if value < last {
            return Err("STEP: knots are not ordered".into());
        }
        if value == last {
            *multiplicities.last_mut().ok_or("STEP: missing knot")? += 1;
        } else {
            values.push(real(value)?);
            multiplicities.push(1);
        }
        last = value;
    }
    Ok((
        multiplicities
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(","),
        values.join(","),
    ))
}

fn real(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("STEP: non-finite coordinate".into());
    }
    let mut value = value.to_string();
    if !value.contains('.') {
        value.push('.');
    }
    Ok(value)
}

fn references(ids: &[u32]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",")
}
fn flag(value: bool) -> &'static str {
    if value { ".T." } else { ".F." }
}
fn step_string(value: &str) -> String {
    format!(
        "'{}'",
        value
            .chars()
            .map(
                |character| if character.is_ascii() && !character.is_control() {
                    character
                } else {
                    '?'
                }
            )
            .collect::<String>()
            .replace('\'', "''")
    )
}
fn status_ok(status: i64) -> Result<(), String> {
    if status != 1000 {
        return Err(format!("SMLib STEP geometry query failed: {status}"));
    }
    Ok(())
}
