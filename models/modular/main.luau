--!strict
local bracket = require("./bracket")
local width: number = cad.number(30, { label = "Width", unit = "mm", min = 10, max = 80 })
local height: number = cad.number(24, { label = "Height", unit = "mm", min = 10, max = 80 })
cad.show("Reusable bracket", bracket(width, height, 3))
