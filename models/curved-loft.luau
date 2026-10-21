local height = cad.number(48, { label = "Height", min = 30, max = 90, unit = "mm" })
local body_radius = cad.number(14, { label = "Body radius", min = 10, max = 22, unit = "mm" })
local neck_radius = cad.number(7, { label = "Neck radius", min = 4, max = 10, unit = "mm" })

local bottle = cad.loft({
	{ profile = cad.circle({ radius = body_radius }), z = 0 },
	{ profile = cad.circle({ radius = body_radius }), z = height * 0.35 },
	{ profile = cad.circle({ radius = body_radius * 0.8 }), z = height * 0.7 },
	{ profile = cad.circle({ radius = neck_radius }), z = height },
}, { degree = 3 })
cad.show("curved bottle blank", bottle)

local function rounded(width: number, depth: number, radius: number)
	return cad.path({
		start = { x = radius, y = 0 },
		segments = {
			{ to = { x = width - radius, y = 0 } },
			{ to = { x = width, y = radius }, center = { x = width - radius, y = radius } },
			{ to = { x = width, y = depth - radius } },
			{ to = { x = width - radius, y = depth }, center = { x = width - radius, y = depth - radius } },
			{ to = { x = radius, y = depth } },
			{ to = { x = 0, y = depth - radius }, center = { x = radius, y = depth - radius } },
			{ to = { x = 0, y = radius } },
			{ to = { x = radius, y = 0 }, center = { x = radius, y = radius } },
		},
	})
end

local transition = cad.loft({
	{ profile = rounded(28, 20, 4), z = 0, x = 40 },
	{ profile = rounded(18, 14, 3), z = height * 0.6, x = 45, y = 3 },
})
cad.show("rounded transition", transition)
