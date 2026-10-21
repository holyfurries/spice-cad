--!strict
local size: number = cad.number(40, { label = "Size", unit = "mm", min = 20, max = 100, step = 1 })
local radius: number = cad.number(2, { label = "Corner radius", unit = "mm", min = 0, max = 9, step = 0.5 })

assert(size > 0, "Size must be positive")
assert(radius >= 0 and radius < size / 2, "Corner radius must be smaller than half the size")

local cube = cad.box({ width = size, depth = size, height = size })
if radius > 0 then
	cube = cad.fillet(cube, { radius = radius })
end
cad.show("Cube", cube)
