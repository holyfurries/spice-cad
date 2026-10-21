--!strict
local width: number = cad.number(60, { label = "Width", unit = "mm" })
local depth: number = cad.number(30, { label = "Depth", unit = "mm" })
local height: number = cad.number(8, { label = "Height", unit = "mm" })
local bevel_width: number = cad.number(4, { label = "Bevel width", unit = "mm" })
local hole_radius: number = cad.number(3, { label = "Hole radius", unit = "mm" })

assert(bevel_width < math.min(width, depth) / 2, "Bevel width must fit within the plate")
assert(hole_radius < math.min(width, depth) / 4, "Hole needs surrounding material")

local part = cad.box({ width = width, depth = depth, height = height })
part = cad.subtract(
	part,
	cad.cylinder({
		radius = hole_radius,
		height = height + 2,
		x = width / 2,
		y = depth / 2,
		z = -1,
	})
)
local front_right = cad.edge(part, {
	name = "front_right",
	from = { x = width, y = 0, z = 0 },
	to = { x = width, y = 0, z = height },
})
local front_left = cad.edge(part, {
	name = "front_left",
	from = { x = 0, y = 0, z = 0 },
	to = { x = 0, y = 0, z = height },
})
cad.show("Selected edge chamfers", cad.chamfer(part, { width = bevel_width, edges = { front_right, front_left } }))
