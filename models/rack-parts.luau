--!strict
local separation: number = cad.number(12, { label = "Part separation", min = 0, max = 100, step = 1, unit = "mm" })
local width: number = cad.number(310, { label = "Rack width", min = 100, max = 480, step = 1, unit = "mm" })
local steps: number = cad.number(5, { label = "Steps", min = 3, max = 8, step = 1 })
local rise: number = cad.number(42, { label = "Step rise", min = 10, max = 80, step = 1, unit = "mm" })
local wall: number = cad.number(3, { label = "Wall thickness", min = 1, max = 10, step = 0.1, unit = "mm" })
local lip: number = cad.number(8, { label = "Lip height", min = 1, max = 20, step = 0.5, unit = "mm" })
local jar_diameter: number = cad.number(50, { label = "Jar diameter", min = 20, max = 100, step = 1, unit = "mm" })
local jar_spacing: number = cad.number(1.5, { label = "Jar spacing", min = 0.5, max = 10, step = 0.1, unit = "mm" })
local joint_clearance: number =
	cad.number(0.2, { label = "Joint clearance", min = 0.05, max = 0.5, step = 0.01, unit = "mm" })
local tread_depth = jar_diameter + 2 * jar_spacing + wall
local depth = steps * tread_depth + wall
local front_steps = math.ceil(steps / 2)
local front_depth = front_steps * tread_depth
local front_height = (front_steps - 1) * rise + wall + lip
local height = (steps - 1) * rise + wall + lip
local seam = width / 2
local tab_tip: typeof(cad.face(cad.box({ width = 1, depth = 1, height = 1 }), { name = "type", normal = "x+" }))? = nil

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
		if side == 0 and section == 0 then
			local tread = cad.face(part, {
				name = "First tread",
				normal = "z+",
				at = wall,
				near = { x = seam / 2, y = tread_depth / 2, z = wall },
			})
			local lip_top = cad.face(part, {
				name = "First lip top",
				normal = "z+",
				at = wall + lip,
				near = { x = seam / 2, y = wall / 2, z = wall + lip },
			})
			cad.measure("Lip height", { from = tread, to = lip_top, parameter = "lip" })
			local front = cad.face(part, {
				name = "First lip inside",
				normal = "y+",
				at = wall,
				near = { x = seam / 2, y = wall, z = wall + lip / 2 },
			})
			local rear = cad.face(part, {
				name = "Second riser front",
				normal = "y-",
				at = tread_depth,
				near = {
					x = seam / 2,
					y = tread_depth,
					z = wall + lip / 2,
				},
			})
			cad.measure("Usable tread depth", { from = front, to = rear, parameter = "jar_diameter" })
			local outer = cad.face(part, {
				name = "Outer side wall",
				normal = "x-",
				at = 0,
				near = { x = 0, y = tread_depth / 2, z = wall + lip / 2 },
			})
			local inner = cad.face(part, {
				name = "Inner side wall",
				normal = "x+",
				at = wall,
				near = { x = wall, y = tread_depth / 2, z = wall + lip / 2 },
			})
			cad.measure("Wall thickness", { from = outer, to = inner, parameter = "wall" })
			tab_tip = cad.face(part, {
				name = "First tab tip",
				normal = "x+",
				at = seam + 6,
				near = { x = seam + 6, y = tread_depth / 2, z = wall / 2 },
			})
		elseif side == 1 and section == 0 then
			local socket = cad.face(part, {
				name = "First socket end",
				normal = "x-",
				at = seam + 6 + joint_clearance,
				near = { x = seam + 6 + joint_clearance, y = tread_depth / 2, z = wall / 2 },
			})
			assert(tab_tip, "Front tab must be defined before its socket")
			cad.measure("Joint tip gap", { from = tab_tip, to = socket, parameter = "joint_clearance" })
		end
		local name = (if section == 0 then "Front" else "Rear") .. (if side == 0 then " left" else " right")
		local color = if side == 0
			then (if section == 0 then { r = 0.25, g = 0.48, b = 0.68 } else { r = 0.30, g = 0.62, b = 0.44 })
			else (if section == 0 then { r = 0.80, g = 0.45, b = 0.18 } else { r = 0.60, g = 0.35, b = 0.65 })
		cad.show(name, part, {
			color = color,
			assembly = { position = { x = side * separation, y = section * separation } },
			print = {
				rotation = { y = if side == 0 then -90 else 90 },
				position = {
					x = (if side == 0 then section_height else 0) + side * 210,
					y = section * 210 - y,
					z = if side == 0 then 0 else width,
				},
			},
		})
	end
end
