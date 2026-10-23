--!strict
local radius: number = cad.number(6, { label = "Outer radius", min = 4, max = 9, unit = "mm" })
local wall: number = cad.number(1.5, { label = "Wall", min = 0.8, max = 3, unit = "mm" })
local bend_radius: number = cad.number(20, { label = "Bend radius", min = 12, max = 28, unit = "mm" })
local length: number = cad.number(45, { label = "Leg length", min = 32, max = 80, unit = "mm" })

local path = { { x = 0, y = 0, z = 0 }, { x = 0, y = 0, z = length }, { x = length, y = 0, z = length } }
local outer = cad.sweep(cad.circle({ radius = radius }), { path = path, bend_radius = bend_radius })
local inner = cad.sweep(cad.circle({ radius = radius - wall }), { path = path, bend_radius = bend_radius })
cad.show("smooth hollow elbow", cad.subtract(outer, inner))
