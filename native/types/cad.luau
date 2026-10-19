type Solid = { _solid: "Solid" }
type Face = { _face: "Face" }
type Profile = { _profile: "Profile" }
type Edge = { _edge: "Edge" }
type Point = { x: number, y: number, z: number }
type BoxOptions = {
	width: number,
	depth: number,
	height: number,
	x: number?,
	y: number?,
	z: number?,
}
local cad: {
	polygon: ({ { x: number, y: number } }) -> Profile,
	extrude: (Profile, { height: number }) -> Solid,
	revolve: (Profile, { angle: number? }) -> Solid,
	cylinder: ({ radius: number, height: number, x: number?, y: number?, z: number? }) -> Solid,
	translate: (Solid, { x: number?, y: number?, z: number? }) -> Solid,
	rotate: (Solid, { axis: "x" | "y" | "z", angle: number }) -> Solid,
	fillet: (Solid, { radius: number, edges: { Edge }? }) -> Solid,
	chamfer: (Solid, { width: number, edges: { Edge }? }) -> Solid,
	edge: (Solid, { name: string, from: Point, to: Point }) -> Edge,
	box: (BoxOptions) -> Solid,
	union: (Solid, Solid) -> Solid,
	subtract: (Solid, Solid) -> Solid,
	show: (string, Solid, { color: { r: number, g: number, b: number }? }?) -> (),
	face: (Solid, { name: string, normal: "x+" | "x-" | "y+" | "y-" | "z+" | "z-", at: number? }) -> Face,
	measure: (string, { from: Face, to: Face, min: number?, max: number? }) -> (),
} =
	nil :: any
