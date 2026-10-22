type Solid = { _solid: "Solid" }
type Face = { _face: "Face" }
type Profile = { _profile: "Profile" }
type Edge = { _edge: "Edge" }
type Material = { _material: "Material" }
type Point = { x: number, y: number, z: number }
type Point2 = { x: number, y: number }
type Placement =
	{ position: { x: number?, y: number?, z: number? }?, rotation: { x: number?, y: number?, z: number? }? }
type BoxOptions = {
	width: number,
	depth: number,
	height: number,
	x: number?,
	y: number?,
	z: number?,
}
type FaceFeatureOptions = {
	face: Face,
	width: number?,
	height: number?,
	profile: Profile?,
	depth: number,
	u: number?,
	v: number?,
}
type HoleOptions = {
	face: Face,
	radius: number,
	u: number?,
	v: number?,
	depth: number?,
	counterbore: { radius: number, depth: number }?,
	countersink: { radius: number, angle: number? }?,
}
local cad: {
	material: (
		string,
		{
			base_color: { r: number, g: number, b: number }?,
			specular_roughness: number?,
			base_metalness: number?,
			specular_weight: number?,
			specular_ior: number?,
		}
	) -> Material,
	print_options: ({ minimum_wall: number?, minimum_clearance: number? }) -> (),
	number: (number, { label: string?, unit: string?, min: number?, max: number?, step: number? }) -> number,
	toggle: (boolean, { label: string? }) -> boolean,
	polygon: ({ { x: number, y: number } }) -> Profile,
	circle: ({ radius: number, x: number?, y: number? }) -> Profile,
	path: ({ start: Point2, segments: { { to: Point2, center: Point2?, clockwise: boolean? } } }) -> Profile,
	profile: ({ loops: { Profile } }) -> Profile,
	extrude: (Profile, { height: number }) -> Solid,
	revolve: (Profile, { angle: number? }) -> Solid,
	sweep: (Profile, { path: { Point }, bend_radius: number? }) -> Solid,
	loft: ({ { profile: Profile, z: number, x: number?, y: number? } }, { degree: number }?) -> Solid,
	shell: (Solid, { thickness: number, open: Face? }) -> Solid,
	cylinder: ({ radius: number, height: number, x: number?, y: number?, z: number? }) -> Solid,
	translate: (Solid, { x: number?, y: number?, z: number? }) -> Solid,
	rotate: (Solid, { axis: "x" | "y" | "z", angle: number }) -> Solid,
	fillet: (Solid, { radius: number, edges: { Edge }? }) -> Solid,
	chamfer: (Solid, { width: number, edges: { Edge }? }) -> Solid,
	edge: (Solid, { name: string, from: Point, to: Point }) -> Edge,
	hole: (Solid, HoleOptions) -> Solid,
	pocket: (Solid, FaceFeatureOptions) -> Solid,
	boss: (Solid, FaceFeatureOptions) -> Solid,
	box: (BoxOptions) -> Solid,
	union: (Solid, Solid) -> Solid,
	subtract: (Solid, Solid) -> Solid,
	show: (string, Solid, { material: Material?, assembly: Placement?, print: Placement? }?) -> (),
	face: (Solid, { name: string, normal: "x+" | "x-" | "y+" | "y-" | "z+" | "z-", at: number?, near: Point? }) -> Face,
	measure: (string, { from: Face, to: Face, min: number?, max: number?, parameter: string? }) -> (),
} =
	nil :: any
