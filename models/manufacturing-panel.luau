--!strict
local thickness = cad.number(8, { label = "Panel thickness", unit = "mm", min = 6, max = 12 })
local part = cad.box({ width = 80, depth = 60, height = thickness })
local recess = cad.polygon({
	{ x = -24, y = -18 },
	{ x = 0, y = -18 },
	{ x = 0, y = -10 },
	{ x = -14, y = -10 },
	{ x = -14, y = 0 },
	{ x = -24, y = 0 },
})
local top = cad.face(part, { name = "panel top", normal = "z+", at = thickness })
part = cad.pocket(part, { face = top, profile = recess, depth = 3 })
local support = cad.polygon({ { x = -6, y = -5 }, { x = 7, y = -5 }, { x = -2, y = 8 } })
top = cad.face(part, { name = "support attachment", normal = "z+", at = thickness })
part = cad.boss(part, { face = top, profile = support, depth = 4, u = 15 })
top = cad.face(part, { name = "socket screw seat", normal = "z+", at = thickness })
part = cad.hole(part, { face = top, radius = 2, u = 25, v = 15, counterbore = { radius = 4, depth = 2 } })
top = cad.face(part, { name = "flush screw seat", normal = "z+", at = thickness })
part = cad.hole(part, { face = top, radius = 2, u = 25, v = -15, countersink = { radius = 4, angle = 90 } })
cad.show("manufacturing panel", part)
