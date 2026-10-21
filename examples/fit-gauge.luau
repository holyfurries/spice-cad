--!strict
local first_gap: number =
	cad.number(0.1, { label = "First per-side gap", min = 0.05, max = 0.4, step = 0.05, unit = "mm" })
local gap_step: number =
	cad.number(0.05, { label = "Gap increment", min = 0.025, max = 0.15, step = 0.025, unit = "mm" })
local pin_width: number = cad.number(4, { label = "Pin width", min = 3, max = 6, unit = "mm" })
local pin_depth: number = cad.number(8, { label = "Pin depth", min = 4, max = 10, unit = "mm" })
local minimum_wall: number = cad.number(0.8, { label = "Minimum wall warning", min = 0.4, max = 2, unit = "mm" })
cad.print_options({ minimum_wall = minimum_wall, minimum_clearance = first_gap })

for index = 0, 4 do
	local gap = first_gap + index * gap_step
	local width = pin_width + gap * 2 + 6
	local depth = pin_depth + gap * 2 + 6
	local receiver = cad.box({ width = width, depth = depth, height = 8 })
	local opening =
		cad.box({ width = pin_width + gap * 2, depth = pin_depth + gap * 2, height = 10, x = 3, y = 3, z = -1 })
	receiver = cad.subtract(receiver, opening)
	local pin = cad.box({ width = pin_width, depth = pin_depth, height = 10 })
	local x = index * 20
	cad.show(string.format("Receiver %d - %.3f mm per side", index + 1, gap), receiver, {
		color = { r = 0.3, g = 0.55, b = 0.8 },
		assembly = { position = { x = x } },
		print = { position = { x = x } },
	})
	cad.show(string.format("Pin %d", index + 1), pin, {
		color = { r = 0.85, g = 0.65, b = 0.3 },
		assembly = { position = { x = x + 3 + gap, y = 3 + gap } },
		print = { position = { x = x, y = 24 } },
	})
end
