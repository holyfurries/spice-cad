--!strict
local width: number = 60
local depth: number = 30
local height: number = 8
local radius: number = 4
local hole_radius: number = 3

assert(radius < math.min(width, depth) / 2, "Corner radius must fit within the plate")
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
cad.show("Selected edge fillets", cad.fillet(part, { radius = radius, edges = { front_right, front_left } }))
