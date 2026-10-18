type Solid = { _solid: "Solid" }
type BoxOptions = {
	width: number,
	depth: number,
	height: number,
	x: number?,
	y: number?,
	z: number?,
}
local cad: {
	cylinder: ({ radius: number, height: number, x: number?, y: number?, z: number? }) -> Solid,
	translate: (Solid, { x: number?, y: number?, z: number? }) -> Solid,
	rotate: (Solid, { axis: "x" | "y" | "z", angle: number }) -> Solid,
	fillet: (Solid, { radius: number }) -> Solid,
	box: (BoxOptions) -> Solid,
	union: (Solid, Solid) -> Solid,
	subtract: (Solid, Solid) -> Solid,
	show: (string, Solid) -> (),
} =
	nil :: any
