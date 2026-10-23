--!strict
type JointPlacement = {
	position: { x: number?, y: number?, z: number? }?,
	rotation: { x: number?, y: number?, z: number? }?,
}
local head_size: number = cad.number(1, { label = "Head size", min = 0.8, max = 1.4, step = 0.05 })
local arm_pose: number = cad.number(-12, { label = "Arm pose", unit = "deg", min = -40, max = 40, step = 1 })
local stride: number = cad.number(0, { label = "Stride", unit = "deg", min = -25, max = 25, step = 1 })
local helmet_visible: boolean = cad.toggle(true, { label = "Helmet" })
local shield_visible: boolean = cad.toggle(true, { label = "Shield" })
local spear_visible: boolean = cad.toggle(true, { label = "Spear" })

local cloth =
	cad.material("Storm blue wool", { base_color = { r = 0.055, g = 0.15, b = 0.22 }, specular_roughness = 1 })
local skin = cad.material("Warm ivory", { base_color = { r = 0.76, g = 0.66, b = 0.50 }, specular_roughness = 1 })
local leather =
	cad.material("Dark leather", { base_color = { r = 0.065, g = 0.045, b = 0.028 }, specular_roughness = 1 })
local trousers =
	cad.material("Charcoal trousers", { base_color = { r = 0.10, g = 0.115, b = 0.12 }, specular_roughness = 1 })
local iron = cad.material("Slate iron", { base_color = { r = 0.25, g = 0.32, b = 0.36 }, specular_roughness = 0.9 })
local paint = cad.material("Chalk shield", { base_color = { r = 0.78, g = 0.76, b = 0.67 }, specular_roughness = 1 })
local wood = cad.material("Ash wood", { base_color = { r = 0.24, g = 0.16, b = 0.085 }, specular_roughness = 1 })
local eyes = cad.material("Eyes", { base_color = { r = 0.028, g = 0.025, b = 0.022 }, specular_roughness = 1 })

local function ring(width: number, depth: number)
	local x, y = width / 2, depth / 2
	local corner = math.min(width, depth) * 0.23
	return cad.polygon({
		{ x = -x + corner, y = -y },
		{ x = x - corner, y = -y },
		{ x = x, y = -y + corner },
		{ x = x, y = y - corner },
		{ x = x - corner, y = y },
		{ x = -x + corner, y = y },
		{ x = -x, y = y - corner },
		{ x = -x, y = -y + corner },
	})
end

local function oval(width: number, depth: number, sides: number)
	local points: { { x: number, y: number } } = {}
	for index = 0, sides - 1 do
		local angle = (index + 0.5) * math.pi * 2 / sides
		table.insert(points, { x = math.cos(angle) * width / 2, y = math.sin(angle) * depth / 2 })
	end
	return cad.polygon(points)
end

local tunic = cad.loft({
	{ profile = ring(34, 22), z = 0 },
	{ profile = ring(24, 17), z = 16 },
	{ profile = ring(27, 18), z = 32 },
}, { degree = 1 })
cad.show("Tunic", tunic, { material = cloth, assembly = { position = { z = 22 } }, shading = { mode = "flat" } })
cad.show("Belt", cad.extrude(ring(25, 18), { height = 2 }), {
	material = leather,
	assembly = { position = { z = 37 } },
})

local head = cad.loft({
	{ profile = oval(24 * head_size, 18.4 * head_size, 8), z = 0 },
	{ profile = oval(30 * head_size, 23 * head_size, 8), z = 4 * head_size },
	{ profile = oval(30 * head_size, 23 * head_size, 8), z = 21 * head_size },
	{ profile = oval(24 * head_size, 18.4 * head_size, 8), z = 26 * head_size },
}, { degree = 1 })
cad.show("Head", head, { material = skin, assembly = { position = { z = 53 } }, shading = { mode = "flat" } })
for _, side in { -1, 1 } do
	local name = if side < 0 then "Left" else "Right"
	cad.show(
		name .. " eye",
		cad.box({ width = 1.7 * head_size, depth = 0.9 * head_size, height = 2.4 * head_size, x = -0.85 * head_size }),
		{
			material = eyes,
			assembly = { position = { x = side * 4.3 * head_size, y = -11.2 * head_size, z = 53 + 12 * head_size } },
		}
	)
end

if helmet_visible then
	local helmet = cad.loft({
		{ profile = oval(33 * head_size, 26 * head_size, 12), z = 0 },
		{ profile = oval(32 * head_size, 32 * 26 / 33 * head_size, 12), z = 5 * head_size },
		{ profile = oval(24 * head_size, 24 * 26 / 33 * head_size, 12), z = 11 * head_size },
		{ profile = oval(10 * head_size, 10 * 26 / 33 * head_size, 12), z = 15 * head_size },
	}, { degree = 1 })
	cad.show("Helmet", helmet, {
		material = iron,
		assembly = { position = { z = 53 + 18.5 * head_size } },
		shading = { mode = "flat" },
	})
	cad.show("Helmet rim", cad.extrude(oval(35 * head_size, 28 * head_size, 12), { height = 1.5 * head_size }), {
		material = iron,
		assembly = { position = { z = 53 + 18 * head_size } },
	})
end

for _, side in { -1, 1 } do
	local name = if side < 0 then "Left" else "Right"
	local hip: JointPlacement = { position = { x = side * 7, z = 27 }, rotation = { x = side * stride, z = side * -4 } }
	local leg = cad.loft({
		{ profile = ring(7, 7), z = -23 },
		{ profile = ring(9, 9), z = 0 },
	}, { degree = 1 })
	cad.show(name .. " leg", leg, { material = trousers, assembly = hip })
	local boot = cad.loft({
		{ profile = ring(10, 14), z = -27, y = -2 },
		{ profile = ring(10, 14), z = -23, y = -2 },
		{ profile = ring(8, 9), z = -19 },
	}, { degree = 1 })
	cad.show(name .. " boot", boot, { material = leather, assembly = hip })
	local shoulder: JointPlacement = {
		position = { x = side * 16.5, z = 50 },
		rotation = { x = if side < 0 then arm_pose * 0.35 else arm_pose, y = side * 10 },
	}
	local sleeve = cad.loft({
		{ profile = ring(6, 7), z = -19, y = -2 },
		{ profile = ring(9, 9), z = -6 },
		{ profile = ring(7, 8), z = 0 },
	}, { degree = 1 })
	cad.show(name .. " sleeve", sleeve, { material = cloth, assembly = shoulder })
	local glove = cad.loft({
		{ profile = ring(5, 6), z = -25, y = -2 },
		{ profile = ring(7, 8), z = -23, y = -2 },
		{ profile = ring(6, 7), z = -18, y = -2 },
	}, { degree = 1 })
	cad.show(name .. " glove", glove, { material = leather, assembly = shoulder })
end

local shield_pose: JointPlacement = { position = { x = -16.5, z = 50 }, rotation = { x = arm_pose * 0.35, y = -10 } }
if shield_visible then
	local function shield_profile(scale: number)
		return cad.polygon({
			{ x = -10 * scale, y = 14 * scale },
			{ x = -13 * scale, y = 7 * scale },
			{ x = -11 * scale, y = -9 * scale },
			{ x = 0, y = -18 * scale },
			{ x = 11 * scale, y = -9 * scale },
			{ x = 13 * scale, y = 7 * scale },
			{ x = 10 * scale, y = 14 * scale },
		})
	end
	local rim = cad.translate(
		cad.rotate(cad.extrude(shield_profile(1), { height = 2 }), { axis = "x", angle = 90 }),
		{ y = -7, z = -14 }
	)
	cad.show("Shield rim", rim, { material = leather, assembly = shield_pose })
	local shield = cad.translate(
		cad.rotate(cad.extrude(shield_profile(0.88), { height = 0.7 }), { axis = "x", angle = 90 }),
		{ y = -9, z = -14 }
	)
	cad.show("Shield", shield, { material = paint, assembly = shield_pose })
	local stripe = cad.polygon({
		{ x = -2, y = 12.3 },
		{ x = -1, y = -15.1 },
		{ x = 0, y = -15.84 },
		{ x = 1, y = -15.1 },
		{ x = 2, y = 12.3 },
	})
	cad.show(
		"Shield stripe",
		cad.translate(
			cad.rotate(cad.extrude(stripe, { height = 0.3 }), { axis = "x", angle = 90 }),
			{ y = -9.7, z = -14 }
		),
		{
			material = cloth,
			assembly = shield_pose,
		}
	)
	local boss = cad.extrude(cad.regular_polygon({ radius = 2.4, sides = 6 }), { height = 1.4 })
	cad.show("Shield boss", cad.translate(cad.rotate(boss, { axis = "x", angle = 90 }), { y = -10, z = -13 }), {
		material = iron,
		assembly = shield_pose,
	})
end

local spear_pose: JointPlacement = { position = { x = 16.5, z = 50 }, rotation = { x = arm_pose, y = 10 } }
if spear_visible then
	local shaft = cad.extrude(cad.regular_polygon({ radius = 0.75, sides = 6 }), { height = 91 })
	cad.show("Spear shaft", cad.translate(shaft, { y = -3, z = -45 }), { material = wood, assembly = spear_pose })
	local blade = cad.polygon({ { x = 0, y = 0 }, { x = 2.5, y = 4 }, { x = 0, y = 12 }, { x = -2.5, y = 4 } })
	local tip = cad.translate(
		cad.rotate(cad.extrude(blade, { height = 1.2 }), { axis = "x", angle = 90 }),
		{ y = -2.4, z = 46 }
	)
	cad.show("Spear tip", tip, { material = iron, assembly = spear_pose })
end
