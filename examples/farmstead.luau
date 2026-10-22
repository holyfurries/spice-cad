--!strict
local scale: number = cad.number(1, { label = "Scale", min = 0.25, max = 2, step = 0.05 })
assert(scale > 0, "Scale must be positive")

local function linear(channel: number): number
	local value = channel / 255
	return if value <= 0.04045 then value / 12.92 else ((value + 0.055) / 1.055) ^ 2.4
end

local plaster = cad.material(
	"Plaster",
	{ base_color = { r = linear(238), g = linear(221), b = linear(179) }, roughness = 1, specular = 0 }
)
local terracotta = cad.material(
	"Terracotta",
	{ base_color = { r = linear(160), g = linear(76), b = linear(49) }, roughness = 1, specular = 0 }
)
local wood = cad.material(
	"Wood",
	{ base_color = { r = linear(101), g = linear(76), b = linear(49) }, roughness = 1, specular = 0 }
)

local body = cad.box({
	width = 100 * scale,
	depth = 85 * scale,
	height = 68 * scale,
	x = -50 * scale,
	y = -42.5 * scale,
})
cad.show("Plaster body", body, { material = plaster })

local gable = cad.polygon({
	{ x = -57.5 * scale, y = 0 },
	{ x = 57.5 * scale, y = 0 },
	{ x = 0, y = 40 * scale },
})
local roof = cad.rotate(cad.extrude(gable, { height = 100 * scale }), { axis = "x", angle = 90 })
roof = cad.translate(roof, { y = 50 * scale, z = 68 * scale })
cad.show("Terracotta roof", roof, { material = terracotta })

local door = cad.box({
	width = 23 * scale,
	depth = 2 * scale,
	height = 46 * scale,
	x = -5.5 * scale,
	y = -44.3 * scale,
})
cad.show("Timber door", door, { material = wood })

local chimney = cad.box({
	width = 16 * scale,
	depth = 18 * scale,
	height = 55 * scale,
	x = 21 * scale,
	y = 5 * scale,
	z = 60.5 * scale,
})
cad.show("Plaster chimney", chimney, { material = plaster })
