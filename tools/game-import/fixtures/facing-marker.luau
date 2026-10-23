--!strict
cad.asset({ id = "verification/facing-marker", license = "MIT" })
local red = cad.material("Right", { base_color = { r = 1, g = 0, b = 0 } })
local blue = cad.material("Forward", { base_color = { r = 0, g = 0, b = 1 } })
cad.show("Right arm", cad.box({ width = 40, depth = 4, height = 4, z = 7 }), { material = red })
cad.show("Forward arm", cad.box({ width = 4, depth = 20, height = 4, y = -20, z = 7 }), { material = blue })
cad.show("Up arm", cad.box({ width = 4, depth = 4, height = 60, z = 7 }))
