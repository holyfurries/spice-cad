--!strict
local roughness: number = cad.number(0.3, { label = "Roughness", min = 0, max = 1, step = 0.01 })
local metallic: number = cad.number(1, { label = "Metalness", min = 0, max = 1, step = 0.01 })
local specular: number = cad.number(1, { label = "Specular weight", min = 0, max = 1, step = 0.01 })

local ceramic = cad.material("Ceramic", {
	base_color = { r = 0.72, g = 0.78, b = 0.85 },
	roughness = roughness,
	specular = specular,
})
local bronze = cad.material("Bronze", {
	base_color = { r = 0.66, g = 0.36, b = 0.12 },
	roughness = roughness,
	metallic = metallic,
})
local matte = cad.material("Matte plaster", {
	base_color = { r = 0.72, g = 0.65, b = 0.48 },
	roughness = 1,
	specular = 0,
})

local block = cad.fillet(cad.box({ width = 30, depth = 30, height = 30 }), { radius = 4 })
cad.show("Ceramic", block, { material = ceramic })
cad.show("Bronze", cad.translate(block, { x = 40 }), { material = bronze })
cad.show("Plaster", cad.translate(block, { x = 80 }), { material = matte })
