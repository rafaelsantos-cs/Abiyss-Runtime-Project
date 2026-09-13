#!/usr/bin/env julia

using JSON3
include(joinpath(@__DIR__, "..", "src", "AbiyssAnalyze.jl"))
using .AbiyssAnalyze

function main(args)
    length(args) == 1 || error("usage: analyze.jl <audit.jsonl>")
    events = load_events(args[1])
    println(JSON3.write(summarize(events)))
end

main(ARGS)
