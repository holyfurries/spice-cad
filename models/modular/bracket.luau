--!strict
local foot = require("./components/foot")
return function(width: number, height: number, thickness: number)
	local base = foot(width, 24, thickness)
	local back = cad.translate(foot(width, thickness, height), { z = thickness })
	return cad.union(base, back)
end
