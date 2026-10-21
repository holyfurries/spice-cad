--!strict
local outer_radius = cad.number(5, { label = "Outer radius", unit = "mm", min = 2, max = 10 })
local wall = cad.number(1.5, { label = "Wall", unit = "mm", min = 0.8, max = 3 })
local length = cad.number(30, { label = "Leg length", unit = "mm", min = 21, max = 100 })
assert(wall < outer_radius, "Wall must leave an open bore")
local path = {
	{ x = 0, y = 0, z = 0 },
	{ x = 0, y = 0, z = length },
	{ x = length, y = 0, z = length },
}
local outer = cad.sweep(cad.circle({ radius = outer_radius }), { path = path })
local inner = cad.sweep(cad.circle({ radius = outer_radius - wall }), { path = path })
cad.show("mitered-pipe", cad.subtract(outer, inner))
