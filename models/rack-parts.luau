--!strict
local print_layout: boolean = false
local separation: number = 12
local width: number = 310
local steps: number = 5
local rise: number = 42
local wall: number = 3
local lip: number = 8
local jar_diameter: number = 50
local jar_spacing: number = 1.5
local joint_clearance: number = 0.2
local tread_depth = jar_diameter + 2 * jar_spacing + wall
local depth = steps * tread_depth + wall
local front_steps = math.ceil(steps / 2)
local front_depth = front_steps * tread_depth
local front_height = (front_steps - 1) * rise + wall + lip
local height = (steps - 1) * rise + wall + lip
local seam = width / 2

assert(steps >= 3 and steps <= 8 and steps == math.floor(steps), "Use 3–8 steps")
assert(front_height >= 36, "Front panel needs at least 36 mm for the joints")
assert(joint_clearance >= 0.05 and joint_clearance <= 0.5, "Joint clearance must be 0.05–0.5 mm")
assert(separation >= 0 and separation <= 100, "Separation must be 0–100 mm")

local function dovetail(clearance: number, thickness: number): typeof(cad.box({ width = 1, depth = 1, height = 1 }))
	return cad.extrude(
		cad.polygon({
			{ x = -1, y = -4 - clearance },
			{ x = 6 + clearance, y = -8 - clearance },
			{ x = 6 + clearance, y = 8 + clearance },
			{ x = -1, y = 4 + clearance },
		}),
		{ height = thickness }
	)
end

for side = 0, 1 do
	for section = 0, 1 do
		local x = side * seam
		local y = if section == 0 then 0 else front_depth
		local section_depth = if section == 0 then front_depth else depth - front_depth
		local section_height = if section == 0 then front_height else height
		local first_step = if section == 0 then 0 else front_steps
		local last_step = if section == 0 then front_steps - 1 else steps - 1
		local outer_x = if side == 0 then 0 else width - wall
		local part = cad.box({ width = seam, depth = section_depth, height = wall, x = x, y = y })
		for index = first_step, last_step do
			local step_y = index * tread_depth
			local z = index * rise
			local side_height = z + wall + lip
			part = cad.union(
				part,
				cad.box({ width = wall, depth = tread_depth, height = side_height, x = outer_x, y = step_y })
			)
			if index > 0 then
				part = cad.union(
					part,
					cad.box({ width = seam, depth = tread_depth, height = wall, x = x, y = step_y, z = z })
				)
			end
			local bottom = z - math.min(rise, z)
			part = cad.union(
				part,
				cad.box({ width = seam, depth = wall, height = side_height - bottom, x = x, y = step_y, z = bottom })
			)
			for floor_index = 0, (if index == 0 then 0 else 1) do
				local floor_z = if floor_index == 0 then 0 else z
				local clearance = if side == 0 then 0 else joint_clearance
				local joint = dovetail(clearance, wall + (if side == 0 then 0 else 0.2))
				joint = cad.translate(
					joint,
					{ x = seam, y = step_y + tread_depth / 2, z = floor_z - (if side == 0 then 0 else 0.1) }
				)
				part = if side == 0 then cad.union(part, joint) else cad.subtract(part, joint)
			end
		end
		if section == 1 then
			part = cad.union(part, cad.box({ width = seam, depth = wall, height = height, x = x, y = depth - wall }))
		end
		for _, fraction in { 0.25, 0.75 } do
			local clearance = if section == 0 then 0 else joint_clearance
			local joint = dovetail(clearance, wall + (if section == 0 then 0 else 0.2))
			joint = cad.rotate(joint, { axis = "x", angle = 90 })
			joint = cad.rotate(joint, { axis = "z", angle = 90 })
			joint = cad.translate(
				joint,
				{ x = outer_x - (if section == 0 then 0 else 0.1), y = front_depth, z = front_height * fraction }
			)
			part = if section == 0 then cad.union(part, joint) else cad.subtract(part, joint)
		end
		if print_layout then
			part = cad.rotate(part, { axis = "y", angle = if side == 0 then -90 else 90 })
			part = cad.translate(part, {
				x = (if side == 0 then section_height else 0) + side * 210,
				y = section * 210 - y,
				z = if side == 0 then 0 else width,
			})
		else
			part = cad.translate(part, { x = side * separation, y = section * separation })
		end
		local name = (if section == 0 then "Front" else "Rear") .. (if side == 0 then " left" else " right")
		local color = if side == 0
			then (if section == 0 then { r = 0.25, g = 0.48, b = 0.68 } else { r = 0.30, g = 0.62, b = 0.44 })
			else (if section == 0 then { r = 0.80, g = 0.45, b = 0.18 } else { r = 0.60, g = 0.35, b = 0.65 })
		cad.show(name, part, { color = color })
	end
end
