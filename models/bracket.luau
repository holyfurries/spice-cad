--!strict
local width: number = cad.number(120, { label = "Width", unit = "mm" })
local depth: number = cad.number(60, { label = "Depth", unit = "mm" })
local wall: number = cad.number(6, { label = "Wall thickness", unit = "mm" })

local base = cad.box({ width = width, depth = depth, height = wall })
local back = cad.box({ width = width, depth = wall, height = 40 })
local opening = cad.box({ width = 30, depth = wall + 2, height = 20, x = 45, y = -1, z = 14 })

cad.show("bracket", cad.subtract(cad.union(base, back), opening))
