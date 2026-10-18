--!strict
local width: number = 120
local depth: number = 60
local wall: number = 6

local base = cad.box({ width = width, depth = depth, height = wall })
local back = cad.box({ width = width, depth = wall, height = 40 })
local opening = cad.box({ width = 30, depth = wall + 2, height = 20, x = 45, y = -1, z = 14 })

cad.show("bracket", cad.subtract(cad.union(base, back), opening))
