--!strict
local width: number = 310
local steps: number = 5
local rise: number = 42
local wall: number = 3
local lip: number = 8
local jar_diameter: number = 50
local jar_spacing: number = 1.5
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
