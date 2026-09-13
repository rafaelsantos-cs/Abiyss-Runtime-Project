module AbiyssAnalyze

using JSON3

export load_events, event_counts, summarize

const MAX_LINE_BYTES = 512 * 1024

function load_events(path::AbstractString; max_events::Integer = 100_000)
    max_events < 1 && throw(ArgumentError("max_events must be positive"))
    events = Vector{Any}()
    open(path, "r") do io
        for line in eachline(io; keep=true)
            ncodeunits(line) > MAX_LINE_BYTES && throw(ArgumentError("audit line exceeds configured limit"))
            isempty(strip(line)) && continue
            push!(events, JSON3.read(line))
            length(events) >= max_events && break
        end
    end
    events
end

function event_counts(events)
    counts = Dict{String, Int}()
    for event in events
        name = try
            String(event.event)
        catch
            "<missing>"
        end
        counts[name] = get(counts, name, 0) + 1
    end
    counts
end

function summarize(events)
    counts = event_counts(events)
    ordered = sort!(collect(counts), by = pair -> (-pair.second, pair.first))
    return Dict(
        "events" => length(events),
        "unique_event_types" => length(counts),
        "top_events" => ordered[1:min(length(ordered), 20)],
    )
end

end
