--!strict
local bore_radius: number = cad.number(5, { label = "Bore radius", unit = "mm" })
local stem_radius: number = cad.number(10, { label = "Stem radius", unit = "mm" })
local flange_radius: number = cad.number(16, { label = "Flange radius", unit = "mm" })
local flange_height: number = cad.number(4, { label = "Flange height", unit = "mm" })
local height: number = cad.number(20, { label = "Height", unit = "mm" })
local sweep_angle: number = cad.number(360, { label = "Sweep angle", min = 1, max = 360, step = 1, unit = "°" })

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
