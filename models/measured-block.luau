--!strict
local width: number = cad.number(40, { label = "Width", unit = "mm" })
local depth: number = cad.number(30, { label = "Depth", unit = "mm" })
local height: number = cad.number(20, { label = "Height", unit = "mm" })

local part = cad.box({ width = width, depth = depth, height = height })
local top = cad.face(part, { name = "top", normal = "z+" })
local bottom = cad.face(part, { name = "bottom", normal = "z-" })
cad.measure("height", { from = bottom, to = top, min = 2, max = 25 })
cad.show("Measured block", part)
