include("WarpPigs.jl")
using .WarpPigs

library = get(ENV, "WARPIGS_NATIVE_LIB", "")
isempty(library) && error("WARPIGS_NATIVE_LIB must point to libwarpigs_core.so")

engine = WarpPigs.Engine(library, UInt64(128), UInt64(10_000), UInt64(42))
try
    @assert WarpPigs.configuration_count(engine) == 81
    @assert WarpPigs.population(engine) == 128
    first_config = WarpPigs.configuration(engine, UInt64(0))
    @assert length(first_config) == 4
    @assert all(c -> c in ['0', '1', '2'], collect(first_config))
    WarpPigs.step!(engine, UInt32(0)) # PREPARE
    WarpPigs.step!(engine, UInt32(1)) # START
    @assert WarpPigs.tick(engine) == 2
    bins = WarpPigs.configuration_histogram(engine)
    @assert length(bins) == 81
    @assert sum(bins) == 128
finally
    WarpPigs.close(engine)
    WarpPigs.close(engine) # idempotence regression
end

println("WarPigs Julia smoke: PASS")
