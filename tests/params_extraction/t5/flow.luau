--!nolint FunctionUnused
type StartParams = {
    action: "start",
}

type StopParams = {
    action: "stop",
}

type Params = StartParams | StopParams

function main(params: Params)
    
end