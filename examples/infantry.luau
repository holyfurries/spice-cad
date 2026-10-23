--!strict
local head_size: number = cad.number(1, { label = "Head size", min = 0.8, max = 1.4, step = 0.05 })
local body_width: number = cad.number(18.6, { label = "Body width", unit = "mm", min = 14, max = 24, step = 0.2 })
local body_height: number = cad.number(26.3, { label = "Body height", unit = "mm", min = 22, max = 32, step = 0.1 })
local heading: number = cad.number(0, { label = "Heading", unit = "deg", min = -180, max = 180, step = 1 })
local lean: number = cad.number(0, { label = "Body lean", unit = "deg", min = -15, max = 15, step = 1 })
local grip: number = cad.number(-8, { label = "Grip pose", unit = "deg", min = -40, max = 40, step = 1 })
local helmet_visible: boolean = cad.toggle(true, { label = "Helmet" })
local shield_visible: boolean = cad.toggle(true, { label = "Shield" })
local spear_visible: boolean = cad.toggle(true, { label = "Spear" })

local ivory = cad.material("Porcelain guard", {
	base_color = { r = 0.72, g = 0.83, b = 0.92 },
	specular_roughness = 0.9,
})
local trim = cad.material("Blue bindings", {
	base_color = { r = 0.025, g = 0.09, b = 0.15 },
	specular_roughness = 1,
})
local ash = cad.material("Ash shaft", {
	base_color = { r = 0.24, g = 0.18, b = 0.11 },
	specular_roughness = 1,
})

local unit = cad.group("Unit rig", { assembly = { rotation = { z = heading } } })
local body_pose =
	cad.group("Body pose", { parent = unit, assembly = { position = { z = 14 }, rotation = { x = lean } } })
local head_mount =
	cad.group("Head mount", { parent = body_pose, assembly = { position = { z = body_height + 9.3 - 14 } } })
local left_grip = cad.group("Left grip", {
	parent = body_pose,
	assembly = { position = { x = -body_width * 0.5 - 2.7, y = -2 }, rotation = { x = grip * 0.35, y = -8 } },
})
local right_grip = cad.group("Right grip", {
	parent = body_pose,
	assembly = { position = { x = body_width * 0.5 + 2.7, y = -2 }, rotation = { x = grip, y = 8 } },
})

local shoulder_sections = {
	{ height = 0, radius = 1 },
	{ height = 0.707, radius = 1 },
	{ height = 0.806, radius = 0.977 },
	{ height = 0.900, radius = 0.866 },
	{ height = 0.954, radius = 0.715 },
	{ height = 1, radius = 0.459 },
}
local sections = {}
for _, section in shoulder_sections do
	table.insert(sections, {
		profile = cad.regular_polygon({ radius = body_width * 0.5 * section.radius, sides = 16 }),
		z = body_height * section.height,
	})
end
local body = cad.loft(sections, { degree = 1 })
cad.show("Body", cad.translate(body, { z = -14 }), {
	parent = body_pose,
	material = ivory,
	shading = { mode = "smooth", crease_angle = 60 },
})
cad.show("Head", cad.sphere({ radius = 7.5 * head_size }), {
	parent = head_mount,
	material = ivory,
	shading = { mode = "smooth", crease_angle = 60 },
})
local function hand()
	return cad.box({ width = 4.2, depth = 4.2, height = 4.2, x = -2.1, y = -2.1, z = -2.1 })
end
cad.show("Left hand", hand(), { parent = left_grip, material = ivory })
cad.show("Right hand", hand(), { parent = right_grip, material = ivory })

local function ring(radius: number)
	local points = {}
	for index = 0, 11 do
		local angle = index * math.pi / 6
		table.insert(points, { x = radius * math.cos(angle), y = radius * 0.94 * math.sin(angle) })
	end
	return cad.polygon(points)
end
if helmet_visible then
	local helmet = cad.loft({
		{ profile = ring(10.3 * head_size), z = 0 },
		{ profile = ring(11.7 * head_size), z = 4.1 * head_size },
		{ profile = ring(9.7 * head_size), z = 11.1 * head_size },
		{ profile = ring(5.2 * head_size), z = 15.1 * head_size },
		{ profile = ring(0.6 * head_size), z = 18.1 * head_size },
	}, { degree = 1 })
	helmet = cad.subtract(
		helmet,
		cad.box({
			width = 14 * head_size,
			depth = 10 * head_size,
			height = 4.5 * head_size,
			x = -7 * head_size,
			y = -14 * head_size,
			z = -0.4 * head_size,
		})
	)
	local cheek_profile = cad.polygon({
		{ x = -8 * head_size, y = 0.5 * head_size },
		{ x = -7 * head_size, y = -3 * head_size },
		{ x = -5 * head_size, y = -5.3 * head_size },
		{ x = -2 * head_size, y = -4.8 * head_size },
		{ x = -head_size, y = 0.5 * head_size },
	})
	local cheek = cad.extrude(cheek_profile, { height = 1.2 * head_size })
	cheek = cad.rotate(cad.rotate(cheek, { axis = "x", angle = 90 }), { axis = "z", angle = 90 })
	helmet = cad.union(helmet, cad.translate(cheek, { x = 6.9 * head_size }))
	helmet = cad.union(helmet, cad.translate(cheek, { x = -8.1 * head_size }))
	cad.show("Helmet", helmet, {
		parent = head_mount,
		material = ivory,
		assembly = { position = { z = -3.1 * head_size } },
		shading = { mode = "flat" },
	})
	cad.show("Helmet binding", cad.extrude(ring(10.6 * head_size), { height = 0.65 * head_size }), {
		parent = head_mount,
		material = trim,
		assembly = { position = { z = -3.6 * head_size } },
	})
end

if shield_visible then
	local function shield_profile(scale: number)
		return cad.polygon({
			{ x = -5.5 * scale, y = 8 * scale },
			{ x = -7 * scale, y = 4 * scale },
			{ x = -6 * scale, y = -5 * scale },
			{ x = 0, y = -10 * scale },
			{ x = 6 * scale, y = -5 * scale },
			{ x = 7 * scale, y = 4 * scale },
			{ x = 5.5 * scale, y = 8 * scale },
		})
	end
	local rim = cad.rotate(cad.extrude(shield_profile(1), { height = 1 }), { axis = "x", angle = 90 })
	cad.show("Shield binding", rim, { parent = left_grip, material = trim, assembly = { position = { y = -2 } } })
	local face = cad.rotate(cad.extrude(shield_profile(0.86), { height = 0.5 }), { axis = "x", angle = 90 })
	cad.show("Shield", face, { parent = left_grip, material = ivory, assembly = { position = { y = -3 } } })
	cad.show("Shield boss", cad.sphere({ radius = 1.4 }), {
		parent = left_grip,
		material = ivory,
		assembly = { position = { y = -3.5 } },
	})
end
if spear_visible then
	local shaft = cad.extrude(cad.regular_polygon({ radius = 0.55, sides = 6 }), { height = 39 })
	cad.show("Spear shaft", shaft, { parent = right_grip, material = ash, assembly = { position = { z = -12 } } })
	local blade = cad.polygon({ { x = 0, y = 0 }, { x = 1.9, y = 3 }, { x = 0, y = 9 }, { x = -1.9, y = 3 } })
	local tip = cad.rotate(cad.extrude(blade, { height = 0.8 }), { axis = "x", angle = 90 })
	cad.show("Spear tip", tip, { parent = right_grip, material = ivory, assembly = { position = { y = 0.4, z = 27 } } })
end
