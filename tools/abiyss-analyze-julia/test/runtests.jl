using Test
using JSON3
include(joinpath(@__DIR__, "..", "src", "AbiyssAnalyze.jl"))
using .AbiyssAnalyze

@testset "AbiyssAnalyze" begin
    path, io = mktemp()
    try
        write(io, "{\"event\":\"query.enqueued\"}\n")
        write(io, "{\"event\":\"query.enqueued\"}\n")
        write(io, "{\"event\":\"query.completed\"}\n")
        close(io)

        events = load_events(path)
        @test length(events) == 3
        counts = event_counts(events)
        @test counts["query.enqueued"] == 2
        @test counts["query.completed"] == 1
        summary = summarize(events)
        @test summary["events"] == 3
        @test summary["unique_event_types"] == 2
    finally
        rm(path; force=true)
    end
end
