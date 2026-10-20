--!strict
local width: number = cad.number(100, { label = "Width", unit = "mm" })
local depth: number = cad.number(50, { label = "Depth", unit = "mm" })
local height: number = cad.number(40, { label = "Height", unit = "mm" })
local wall: number = cad.number(6, { label = "Wall thickness", unit = "mm" })
local edge_radius: number = cad.number(1, { label = "Edge radius", unit = "mm" })
local hole_radius: number = cad.number(3, { label = "Hole radius", unit = "mm" })
local hole_inset: number = cad.number(15, { label = "Hole inset", unit = "mm" })

assert(width > 2 * hole_inset + 2 * hole_radius)
assert(depth > hole_inset + hole_radius + wall + edge_radius)
assert(height > hole_inset + hole_radius + wall + edge_radius)
assert(hole_inset > hole_radius + edge_radius)
assert(edge_radius < wall / 2)

local base = cad.box({ width = width, depth = depth, height = wall })
local upright = cad.box({ width = width, depth = wall, height = height })
local bracket = cad.fillet(cad.union(base, upright), { radius = edge_radius })

for index, u in { hole_inset - width / 2, width / 2 - hole_inset } do
	local base_face = cad.face(bracket, { name = "base " .. index, normal = "z+", at = wall })
	bracket = cad.hole(bracket, {
		face = base_face,
		radius = hole_radius,
		u = u,
		v = (depth - wall) / 2 - hole_inset,
	})
	local upright_face = cad.face(bracket, { name = "upright " .. index, normal = "y-", at = 0 })
	bracket = cad.hole(bracket, { face = upright_face, radius = hole_radius, u = u, v = height / 2 - hole_inset })
end

cad.show("mounting bracket", bracket)
