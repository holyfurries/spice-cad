--!strict
local width: number = 64
local depth: number = 42
local arm: number = 10
local web: number = 24
local thickness: number = 6
local hole_radius: number = 2

assert(width > arm + web, "Width must leave room for the diagonal web")
assert(depth > arm + web, "Depth must leave room for the diagonal web")
assert(arm > hole_radius * 2 + 2, "Holes need at least 1 mm of surrounding material")

local outline = cad.polygon({
	{ x = 0, y = 0 },
	{ x = width, y = 0 },
	{ x = width, y = arm },
	{ x = arm + web, y = arm },
	{ x = arm, y = arm + web },
	{ x = arm, y = depth },
	{ x = 0, y = depth },
})
local part = cad.extrude(outline, { height = thickness })
part = cad.subtract(
	part,
	cad.cylinder({
		radius = hole_radius,
		height = thickness + 2,
		x = width - arm / 2,
		y = arm / 2,
		z = -1,
	})
)
part = cad.subtract(
	part,
	cad.cylinder({
		radius = hole_radius,
		height = thickness + 2,
		x = arm / 2,
		y = depth - arm / 2,
		z = -1,
	})
)
cad.show("Extruded bracket", part)
