module WarpPigs

"""Minimal Julia consumer of the canonical WarPigs C ABI.

The native library performs all simulation semantics. Julia is intentionally
limited to analysis and experiment orchestration.
"""

const ABI_VERSION = UInt32(1)
const WP_OK = UInt32(0)

struct Engine
    handle::Ptr{Cvoid}
    library::String
end

function Engine(library::AbstractString, population::UInt64, max_population::UInt64, seed::UInt64)
    handle_ref = Ref{Ptr{Cvoid}}(C_NULL)
    status = ccall((:wp_engine_create, library), UInt32,
        (UInt64, UInt64, UInt64, Ref{Ptr{Cvoid}}),
        population, max_population, seed, handle_ref)
    status == WP_OK || error("wp_engine_create failed with code $status")
    handle_ref[] == C_NULL && error("native engine returned NULL")
    version = ccall((:wp_engine_abi_version, library), UInt32, ())
    version == ABI_VERSION || begin
        ccall((:wp_engine_destroy, library), Cvoid, (Ptr{Cvoid},), handle_ref[])
        error("unsupported WarPigs ABI version $version")
    end
    return Engine(handle_ref[], String(library))
end

function close(engine::Engine)
    engine.handle == C_NULL && return
    ccall((:wp_engine_destroy, engine.library), Cvoid, (Ptr{Cvoid},), engine.handle)
end

population(engine::Engine) = ccall((:wp_engine_population, engine.library), UInt64, (Ptr{Cvoid},), engine.handle)
tick(engine::Engine) = ccall((:wp_engine_tick, engine.library), UInt64, (Ptr{Cvoid},), engine.handle)
configuration_count(engine::Engine) = ccall((:wp_engine_configuration_count, engine.library), UInt32, ())

function configuration(engine::Engine, index::UInt64)
    buffer = Vector{UInt8}(undef, 5)
    status = ccall((:wp_engine_configuration_text, engine.library), UInt32,
        (Ptr{Cvoid}, UInt64, Ptr{UInt8}, Csize_t),
        engine.handle, index, pointer(buffer), length(buffer))
    status == WP_OK || error("configuration lookup failed with code $status")
    return String(buffer[1:4])
end

function step!(engine::Engine, action::UInt32)
    status = ccall((:wp_engine_step, engine.library), UInt32,
        (Ptr{Cvoid}, UInt32), engine.handle, action)
    status == WP_OK || error("step failed with code $status")
    return nothing
end

"""Collect the native 81-bin configuration histogram as a Julia vector."""
function configuration_histogram(engine::Engine)
    bins = zeros(UInt64, 81)
    status = ccall((:wp_engine_configuration_histogram, engine.library), UInt32,
        (Ptr{Cvoid}, Ptr{UInt64}, Csize_t), engine.handle, pointer(bins), length(bins))
    status == WP_OK || error("histogram failed with code $status")
    return bins
end

end # module
