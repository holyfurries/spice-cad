--!strict
local width = cad.number(60, { label = "Width", unit = "mm", min = 40, max = 120 })
local depth = cad.number(40, { label = "Depth", unit = "mm", min = 30, max = 80 })
local height = cad.number(20, { label = "Height", unit = "mm", min = 8, max = 60 })
local wall = cad.number(2, { label = "Wall", unit = "mm", min = 1, max = 4 })

local function outline(w: number, d: number, r: number, inset: number)
	return cad.path({
		start = { x = inset + r, y = inset },
		segments = {
			{ to = { x = inset + w - r, y = inset } },
			{ to = { x = inset + w, y = inset + r }, center = { x = inset + w - r, y = inset + r } },
			{ to = { x = inset + w, y = inset + d - r } },
			{ to = { x = inset + w - r, y = inset + d }, center = { x = inset + w - r, y = inset + d - r } },
			{ to = { x = inset + r, y = inset + d } },
			{ to = { x = inset, y = inset + d - r }, center = { x = inset + r, y = inset + d - r } },
			{ to = { x = inset, y = inset + r } },
			{ to = { x = inset + r, y = inset }, center = { x = inset + r, y = inset + r } },
		},
	})
end

local outer = outline(width, depth, 6, 0)
local inner = outline(width - 2 * wall, depth - 2 * wall, 6 - wall, wall)
local sides = cad.profile({ loops = { outer, inner } })
local floor = cad.profile({
	loops = {
		outer,
		cad.circle({ radius = 1.5, x = 10, y = 10 }),
		cad.circle({ radius = 1.5, x = width - 10, y = 10 }),
		cad.circle({ radius = 1.5, x = width - 10, y = depth - 10 }),
		cad.circle({ radius = 1.5, x = 10, y = depth - 10 }),
	},
})
local base = cad.extrude(floor, { height = wall })
local walls = cad.translate(cad.extrude(sides, { height = height - wall }), { z = wall })
local enclosure = cad.union(base, walls)
cad.show("rounded-enclosure", enclosure, { color = { r = 0.58, g = 0.72, b = 0.88 } })
