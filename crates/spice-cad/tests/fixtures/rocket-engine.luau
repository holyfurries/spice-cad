--!strict
local throat_radius: number = cad.number(8, { label = "Throat radius", unit = "mm" })
local wall: number = cad.number(4, { label = "Wall thickness", unit = "mm" })
local chamber_top: number = cad.number(75, { label = "Chamber top", unit = "mm" })
local chamber_bottom: number = cad.number(30, { label = "Chamber bottom", unit = "mm" })
local bell_radius: number = cad.number(22, { label = "Bell radius", unit = "mm" })

local chamber_section = cad.polygon({
	{ x = throat_radius, y = chamber_bottom },
	{ x = throat_radius + wall, y = chamber_bottom },
	{ x = throat_radius + wall, y = chamber_top },
	{ x = throat_radius, y = chamber_top },
})
local chamber = cad.revolve(chamber_section, {})
local bell_section = cad.polygon({
	{ x = bell_radius, y = 0 },
	{ x = bell_radius + wall, y = 0 },
	{ x = throat_radius + wall, y = chamber_bottom },
	{ x = throat_radius, y = chamber_bottom },
})
local bell = cad.revolve(bell_section, {})
local mount = cad.box({ width = 56, depth = 56, height = 5, x = -28, y = -28, z = chamber_top })
mount = cad.subtract(mount, cad.cylinder({ radius = throat_radius, height = 7, z = chamber_top - 1 }))
cad.show("Combustion chamber", chamber, {
	material = cad.material(
		"Steel",
		{ base_color = { r = 0.24, g = 0.30, b = 0.38 }, base_metalness = 1, specular_roughness = 0.35 }
	),
	print = { position = { x = 60, z = -chamber_bottom } },
})
cad.show("Bell nozzle", bell, {
	material = cad.material(
		"Copper",
		{ base_color = { r = 0.58, g = 0.26, b = 0.10 }, base_metalness = 1, specular_roughness = 0.3 }
	),
	print = {},
})
cad.show("Mount plate", mount, {
	material = cad.material("Paint", { base_color = { r = 0.90, g = 0.50, b = 0.12 }, specular_roughness = 0.6 }),
	print = { position = { x = 120, z = -chamber_top } },
})
