--!strict
local width: number = 40
local depth: number = 30
local height: number = 20

local part = cad.box({ width = width, depth = depth, height = height })
local top = cad.face(part, { name = "top", normal = "z+" })
local bottom = cad.face(part, { name = "bottom", normal = "z-" })
cad.measure("height", { from = bottom, to = top, min = 2, max = 25 })
cad.show("Measured block", part)
