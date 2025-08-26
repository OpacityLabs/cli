--!nolint FunctionUnused
type UnionType = "A" | "B" | "C"

type Params = {
	field1: string?,
	field2: string,
	field999: "A",
	field998: "A" | "B" | "C",
	field3: ("A" | "B" | "C")?, -- if you wish to have a nullable union type, you can wrap it in parentheses and use the ? operator
	field4: number?,
	field5: number,
	field6: boolean?,
	field7: boolean,
	field8: false,
	field9: true,
	field10: UnionType?
}

function main(params: Params)

end
