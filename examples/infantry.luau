--!strict
local head_size: number = cad.number(1, { label = "Head size", min = 0.8, max = 1.4, step = 0.05 })
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
local head_mount = cad.group("Head mount", { parent = body_pose, assembly = { position = { z = 19 } } })
local left_grip = cad.group("Left grip", {
	parent = body_pose,
	assembly = { position = { x = -12, y = -2 }, rotation = { x = grip * 0.35, y = -8 } },
})
local right_grip = cad.group("Right grip", {
	parent = body_pose,
	assembly = { position = { x = 12, y = -2 }, rotation = { x = grip, y = 8 } },
})

local body = cad.capsule({ radius = 9, height = 34, z = -9 })
body = cad.subtract(body, cad.box({ width = 40, depth = 40, height = 10, x = -20, y = -20, z = -10 }))
cad.show("Body", cad.translate(body, { z = -14 }), {
	parent = body_pose,
	material = ivory,
	shading = { mode = "smooth", crease_angle = 60 },
})
cad.show("Head", cad.sphere({ radius = 7 * head_size }), {
	parent = head_mount,
	material = ivory,
	shading = { mode = "smooth", crease_angle = 60 },
})
cad.show("Left hand", cad.sphere({ radius = 1.9 }), { parent = left_grip, material = ivory })
cad.show("Right hand", cad.sphere({ radius = 1.9 }), { parent = right_grip, material = ivory })

local function ring(radius: number)
	return cad.regular_polygon({ radius = radius, sides = 12, rotation = 15 })
end
if helmet_visible then
	local helmet = cad.loft({
		{ profile = ring(8.5 * head_size), z = 0 },
		{ profile = ring(8.3 * head_size), z = 3 * head_size },
		{ profile = ring(5.8 * head_size), z = 7 * head_size },
		{ profile = ring(1.3 * head_size), z = 9 * head_size },
	}, { degree = 1 })
	cad.show("Helmet", helmet, {
		parent = head_mount,
		material = ivory,
		assembly = { position = { z = 3 * head_size } },
		shading = { mode = "flat" },
	})
	cad.show("Helmet binding", cad.extrude(ring(9 * head_size), { height = 0.65 * head_size }), {
		parent = head_mount,
		material = trim,
		assembly = { position = { z = 2.6 * head_size } },
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
