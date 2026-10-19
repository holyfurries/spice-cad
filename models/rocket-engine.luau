--!strict
local print_layout: boolean = cad.toggle(false, { label = "Print layout" })
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
if print_layout then
	chamber = cad.translate(chamber, { x = 60, z = -chamber_bottom })
	mount = cad.translate(mount, { x = 120, z = -chamber_top })
end
cad.show("Combustion chamber", chamber, { color = { r = 0.24, g = 0.30, b = 0.38 } })
cad.show("Bell nozzle", bell, { color = { r = 0.58, g = 0.26, b = 0.10 } })
cad.show("Mount plate", mount, { color = { r = 0.90, g = 0.50, b = 0.12 } })
