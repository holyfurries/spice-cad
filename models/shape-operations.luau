local base = cad.polygon({ { x = -15, y = -10 }, { x = 15, y = -10 }, { x = 15, y = 10 }, { x = -15, y = 10 } })
local top = cad.polygon({ { x = -9, y = -6 }, { x = 9, y = -6 }, { x = 9, y = 6 }, { x = -9, y = 6 } })
local transition = cad.loft({ { profile = base, z = 0 }, { profile = top, z = 24 } })
cad.show("transition", transition)

local section = cad.polygon({ { x = -2, y = -2 }, { x = 2, y = -2 }, { x = 2, y = 2 }, { x = -2, y = 2 } })
local rail =
	cad.sweep(section, { path = { { x = 45, y = 0, z = 0 }, { x = 45, y = 0, z = 30 }, { x = 75, y = 0, z = 30 } } })
cad.show("rail", rail)

local block = cad.box({ width = 30, depth = 20, height = 16, x = -60 })
local opening = cad.face(block, { name = "housing opening", normal = "z+" })
cad.show("housing", cad.shell(block, { thickness = 2, open = opening }))
