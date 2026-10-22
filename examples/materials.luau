--!strict
local specular_roughness: number = cad.number(0.3, { label = "Roughness", min = 0, max = 1, step = 0.01 })
local base_metalness: number = cad.number(1, { label = "Metalness", min = 0, max = 1, step = 0.01 })
local specular_weight: number = cad.number(1, { label = "Specular weight", min = 0, max = 1, step = 0.01 })
local specular_ior: number = cad.number(1.5, { label = "IOR", min = 1, max = 3, step = 0.01 })

local ceramic = cad.material("Ceramic", {
	base_color = { r = 0.72, g = 0.78, b = 0.85 },
	specular_roughness = specular_roughness,
	specular_weight = specular_weight,
	specular_ior = specular_ior,
})
local bronze = cad.material("Bronze", {
	base_color = { r = 0.66, g = 0.36, b = 0.12 },
	specular_roughness = specular_roughness,
	base_metalness = base_metalness,
})
local matte = cad.material("Matte plaster", {
	base_color = { r = 0.72, g = 0.65, b = 0.48 },
	specular_roughness = 1,
	specular_weight = 0,
})

local block = cad.fillet(cad.box({ width = 30, depth = 30, height = 30 }), { radius = 4 })
cad.show("Ceramic", block, { material = ceramic })
cad.show("Bronze", cad.translate(block, { x = 40 }), { material = bronze })
cad.show("Plaster", cad.translate(block, { x = 80 }), { material = matte })
