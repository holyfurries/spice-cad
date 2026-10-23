use crate::placement::Placement;
use mlua::{AnyUserData, Lua, Table, UserData};
use std::{cell::RefCell, rc::Rc};

pub struct Group {
    pub name: String,
    pub attachment: Option<String>,
    pub parent: Option<usize>,
    pub placement: Placement,
}

pub struct Handle(usize);
impl UserData for Handle {}

pub fn parent(options: &Table) -> mlua::Result<Option<usize>> {
    options
        .get::<Option<AnyUserData>>("parent")?
        .map(|value| Ok(value.borrow::<Handle>()?.0))
        .transpose()
}

pub fn world_placements(groups: &[Group]) -> Result<Vec<Placement>, String> {
    assert!(groups.len() <= 32);
    let mut placements = Vec::<Placement>::with_capacity(groups.len());
    let mut depths = [0u32; 32];
    for (index, group) in groups.iter().enumerate() {
        let world = if let Some(parent) = group.parent {
            assert!(parent < index);
            depths[index] = depths[parent] + 1;
            if depths[index] >= 16 {
                return Err("Group hierarchy exceeds 16 levels".into());
            }
            placements[parent].composed(group.placement)?
        } else {
            group.placement
        };
        placements.push(world);
    }
    assert_eq!(placements.len(), groups.len());
    Ok(placements)
}

pub fn register(lua: &Lua, cad: &Table) -> mlua::Result<Rc<RefCell<Vec<Group>>>> {
    let groups = Rc::new(RefCell::new(Vec::<Group>::new()));
    let state = Rc::clone(&groups);
    cad.set(
        "group",
        lua.create_function(move |_, (name, options): (String, Option<Table>)| {
            if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                return Err(mlua::Error::runtime(
                    "Group names must contain 1–80 bytes without control characters",
                ));
            }
            let (parent, placement, attachment) = if let Some(options) = options {
                crate::model::options_fields(&options, &["parent", "assembly", "attachment"])?;
                let attachment: Option<String> = options.get("attachment")?;
                if attachment.as_ref().is_some_and(|role| {
                    role.is_empty() || role.len() > 80 || role.chars().any(char::is_control)
                }) {
                    return Err(mlua::Error::runtime(
                        "Attachment roles must contain 1–80 bytes without control characters",
                    ));
                }
                let parent = parent(&options)?;
                let placement = options
                    .get::<Option<Table>>("assembly")?
                    .map(Placement::from_table)
                    .transpose()?
                    .unwrap_or_default();
                (parent, placement, attachment)
            } else {
                (None, Placement::default(), None)
            };
            let mut groups = state.borrow_mut();
            if groups.len() >= 32 {
                return Err(mlua::Error::runtime("At most 32 groups are supported"));
            }
            if groups.iter().any(|group| group.name == name) {
                return Err(mlua::Error::runtime("Group names must be unique"));
            }
            let index = groups.len();
            if parent.is_some_and(|parent| parent >= index) {
                return Err(mlua::Error::runtime("Group parent must already exist"));
            }
            groups.push(Group {
                name,
                attachment,
                parent,
                placement,
            });
            Ok(Handle(index))
        })?,
    )?;
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec3};

    fn node_matrix(node: &serde_json::Value) -> Mat4 {
        let translation = Vec3::from_array(std::array::from_fn(|index| {
            node["translation"][index].as_f64().unwrap() as f32
        }));
        let rotation = Quat::from_array(std::array::from_fn(|index| {
            node["rotation"][index].as_f64().unwrap() as f32
        }));
        Mat4::from_rotation_translation(rotation, translation)
    }

    #[test]
    fn group_transforms_preserve_world_geometry_print_placement_and_glb_hierarchy()
    -> Result<(), String> {
        let source = "local root=cad.group('root',{assembly={position={x=10,y=20,z=30},rotation={z=90}}});local child=cad.group('grip',{parent=root,assembly={position={x=8,z=5},rotation={x=35}}});cad.show('blade',cad.box({width=2,depth=3,height=4}),{parent=child,assembly={position={x=3,y=-2,z=1},rotation={y=25}},print={}});cad.show('loose',cad.box({width=1,depth=1,height=1}),{print={position={x=30}}})";
        let model = crate::model::compile(source)?;
        let world = Mat4::from_translation(glam::vec3(10.0, 20.0, 30.0))
            * Mat4::from_rotation_z(90f32.to_radians())
            * Mat4::from_translation(glam::vec3(8.0, 0.0, 5.0))
            * Mat4::from_rotation_x(35f32.to_radians())
            * Mat4::from_translation(glam::vec3(3.0, -2.0, 1.0))
            * Mat4::from_rotation_y(25f32.to_radians());
        let point = glam::vec3(1.0, 2.0, 3.0);
        assert!(
            model.parts[0]
                .assembly
                .matrix()
                .transform_point3(point)
                .distance(world.transform_point3(point))
                < 1e-5
        );
        assert_eq!(
            model.parts[0]
                .placement(crate::placement::Layout::Print)
                .matrix(),
            Mat4::IDENTITY
        );
        let kernel = crate::kernel::Kernel::new()?;
        let meshes = model.meshes(&mut model.build(&kernel)?, true)?;
        let bytes = crate::game_asset::glb(&model, &meshes)?;
        let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + size]).map_err(|error| error.to_string())?;
        assert_eq!(json["scenes"][0]["nodes"], serde_json::json!([1, 2]));
        assert_eq!(json["nodes"][2]["children"], serde_json::json!([3]));
        assert_eq!(json["nodes"][3]["children"], serde_json::json!([0]));
        let matrix = node_matrix(&json["nodes"][2])
            * node_matrix(&json["nodes"][3])
            * node_matrix(&json["nodes"][0]);
        let basis = |p: Vec3| Vec3::new(p.x, p.z, -p.y) * 0.001;
        assert!(
            matrix
                .transform_point3(basis(point))
                .distance(basis(world.transform_point3(point)))
                < 1e-7
        );
        let mut body = model.build(&kernel)?.remove(0);
        model.parts[0].assembly.apply(&mut body, false)?;
        let placed = body.mesh()?.mesh;
        for vertex in &placed.vertices {
            assert!(meshes[0].vertices.iter().any(|original| {
                world
                    .transform_point3(Vec3::from_array(original.position))
                    .distance(Vec3::from_array(vertex.position))
                    < 1e-4
            }));
        }
        Ok(())
    }

    #[test]
    fn parented_measurements_follow_nested_rotations_and_depth_is_checked_at_runtime()
    -> Result<(), String> {
        let source = "local root=cad.group('root',{assembly={rotation={y=90}}});local child=cad.group('child',{parent=root,assembly={position={x=12},rotation={x=35}}});local box=cad.box({width=2,depth=3,height=4});local bottom=cad.face(box,{name='bottom',normal='z-'});local top=cad.face(box,{name='top',normal='z+'});cad.measure('height',{from=bottom,to=top,min=3.9,max=4.1});cad.show('box',box,{parent=child})";
        let kernel = crate::kernel::Kernel::new()?;
        let mut cache = crate::model::Cache::new(&kernel);
        let model = cache.compile(source)?;
        cache.prepare(&model)?;
        let annotations = cache.annotations(&model)?;
        assert_eq!(annotations.measurements.len(), 1);
        assert!((annotations.measurements[0].distance_mm - 4.0).abs() < 1e-5);
        let source = "local parent:Group?=nil;for i=1,17 do parent=cad.group(tostring(i),{parent=parent}) end;cad.show('valid',cad.box({width=1,depth=1,height=1}))";
        let error = crate::model::compile(source).err().expect("Depth limit");
        assert!(
            error.contains("Group hierarchy exceeds 16 levels"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn groups_reject_invalid_names_handles_depth_and_composed_positions() {
        for source in [
            "cad.group('same');cad.group('same')",
            "cad.group('')",
            "cad.group('bad',{scale=2})",
            "cad.group('bad',{parent=cad.box({width=1,depth=1,height=1})})",
            "local g=cad.group('root',{assembly={position={x=2000}}});cad.show('far',cad.box({width=1,depth=1,height=1}),{parent=g,assembly={position={x=1}}})",
            "local parent=nil;for i=1,17 do parent=cad.group(tostring(i),{parent=parent}) end",
            "for i=1,33 do cad.group(tostring(i)) end",
        ] {
            let source =
                format!("{source};cad.show('valid',cad.box({{width=1,depth=1,height=1}}))");
            assert!(crate::model::compile(&source).is_err(), "{source}");
        }
    }
}
