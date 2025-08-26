--!nolint FunctionUnused

--[[
	This is a multiline comment for the type Params
]]
type Params = {
	-- this is a comment about the field a
	a: number,
	-- this is a single comment about the field b
	-- this is a secondary single comment about the field b
	b: number,
	--[[ This is a multiline comment
	 about the field c ]]
	c: number
}

function main(params: Params)

end
