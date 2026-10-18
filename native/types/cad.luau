type Solid = { _solid: "Solid" }
type Face = { _face: "Face" }
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
	face: (Solid, { name: string, normal: "x+" | "x-" | "y+" | "y-" | "z+" | "z-", at: number? }) -> Face,
	measure: (string, { from: Face, to: Face, min: number?, max: number? }) -> (),
} =
	nil :: any
