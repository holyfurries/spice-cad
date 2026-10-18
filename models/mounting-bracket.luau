--!strict
local width: number = 100
local depth: number = 50
local height: number = 40
local wall: number = 6
local edge_radius: number = 1
local hole_radius: number = 3
local hole_inset: number = 15

assert(width > 2 * hole_inset + 2 * hole_radius)
assert(depth > hole_inset + hole_radius + wall + edge_radius)
assert(height > hole_inset + hole_radius + wall + edge_radius)
assert(hole_inset > hole_radius + edge_radius)
assert(edge_radius < wall / 2)

local base = cad.box({ width = width, depth = depth, height = wall })
local upright = cad.box({ width = width, depth = wall, height = height })
local bracket = cad.fillet(cad.union(base, upright), { radius = edge_radius })

for _, x in { hole_inset, width - hole_inset } do
	local base_hole = cad.cylinder({ radius = hole_radius, height = wall + 2, x = x, y = depth - hole_inset, z = -1 })
	bracket = cad.subtract(bracket, base_hole)
	local upright_hole = cad.cylinder({ radius = hole_radius, height = wall + 2 })
	upright_hole = cad.rotate(upright_hole, { axis = "x", angle = -90 })
	upright_hole = cad.translate(upright_hole, { x = x, y = -1, z = height - hole_inset })
	bracket = cad.subtract(bracket, upright_hole)
end

cad.show("mounting bracket", bracket)
