--!strict
local bore_radius: number = 5
local stem_radius: number = 10
local flange_radius: number = 16
local flange_height: number = 4
local height: number = 20
local sweep_angle: number = 360

assert(bore_radius > 0, "Bore radius must be positive")
assert(stem_radius > bore_radius, "Stem must leave material around the bore")
assert(flange_radius > stem_radius, "Flange must be wider than the stem")
assert(flange_height > 0 and flange_height < height, "Flange height must fit below the stem")

local section = cad.polygon({
	{ x = bore_radius, y = 0 },
	{ x = flange_radius, y = 0 },
	{ x = flange_radius, y = flange_height },
	{ x = stem_radius, y = flange_height },
	{ x = stem_radius, y = height },
	{ x = bore_radius, y = height },
})
cad.show("Revolved bushing", cad.revolve(section, { angle = sweep_angle }))
