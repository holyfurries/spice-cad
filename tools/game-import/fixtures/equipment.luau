--!strict
cad.asset({ id = "verification/equipment", license = "MIT" })
local body = cad.group("Body", { assembly = { position = { x = 10, z = 20 }, rotation = { z = 35 } } })
local grip = cad.group("Grip", {
	parent = body,
	attachment = "weapon_grip",
	assembly = { position = { x = 4, z = 3 }, rotation = { y = 25 } },
})
local tip = cad.group("Tip", {
	parent = grip,
	attachment = "weapon_tip",
	assembly = { position = { z = 14 }, rotation = { x = 15 } },
})
cad.show("Blade", cad.box({ width = 2, depth = 3, height = 14 }), { parent = grip })
cad.show("Tip marker", cad.box({ width = 3, depth = 4, height = 2 }), { parent = tip })
