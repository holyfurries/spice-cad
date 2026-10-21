--!strict
local width: number = cad.number(310, { label = "Rack width", unit = "mm" })
local steps: number = cad.number(5, { label = "Steps", min = 3, max = 8, step = 1 })
local rise: number = cad.number(42, { label = "Step rise", unit = "mm" })
local wall: number = cad.number(3, { label = "Wall thickness", unit = "mm" })
local lip: number = cad.number(8, { label = "Lip height", unit = "mm" })
local jar_diameter: number = cad.number(50, { label = "Jar diameter", unit = "mm" })
local jar_spacing: number = cad.number(1.5, { label = "Jar spacing", unit = "mm" })
local tread_depth = jar_diameter + 2 * jar_spacing + wall
local depth = steps * tread_depth + wall
local height = (steps - 1) * rise + wall + lip

local rack = cad.box({ width = width, depth = depth, height = wall })
for index = 0, steps - 1 do
	local y = index * tread_depth
	local z = index * rise
	local side_height = z + wall + lip
	for _, x in { 0, width - wall } do
		rack = cad.union(rack, cad.box({ width = wall, depth = tread_depth, height = side_height, x = x, y = y }))
	end
	rack = cad.union(rack, cad.box({ width = width, depth = tread_depth, height = wall, y = y, z = z }))
	local bottom = z - math.min(rise, z)
	rack = cad.union(rack, cad.box({ width = width, depth = wall, height = side_height - bottom, y = y, z = bottom }))
end
rack = cad.union(rack, cad.box({ width = width, depth = wall, height = height, y = depth - wall }))
cad.show("spice rack blank", rack)
