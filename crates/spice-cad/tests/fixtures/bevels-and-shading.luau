--!strict
local bevel: number = cad.number(1, { label = "Bevel width", unit = "mm", min = 0, max = 4, step = 0.1 })
local crease: number = cad.number(40, { label = "Crease angle", unit = "deg", min = 0, max = 180, step = 1 })
local surface = cad.material("Painted metal", {
	base_color = { r = 0.12, g = 0.32, b = 0.45 },
	specular_roughness = 0.35,
})

local block = cad.box({ width = 30, depth = 30, height = 30 })
if bevel > 0 then
	block = cad.chamfer(block, { width = bevel })
end
cad.show("Beveled block", block, { material = surface, shading = { mode = "flat" } })
cad.show("Faceted cylinder", cad.cylinder({ radius = 15, height = 30, x = 55, y = 15 }), {
	material = surface,
	shading = { mode = "flat" },
})
cad.show("Smooth cylinder", cad.cylinder({ radius = 15, height = 30, x = 95, y = 15 }), {
	material = surface,
	shading = { mode = "smooth", crease_angle = crease },
})
