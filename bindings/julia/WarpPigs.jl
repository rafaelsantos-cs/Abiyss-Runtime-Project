module WarpPigs

"""Julia consumer of the canonical WarPigs C ABI.

Julia is intentionally limited to experiment orchestration and statistical
analysis. The native C++ engine owns simulation semantics.
"""

const ABI_VERSION = UInt32(1)
const WP_OK = UInt32(0)
const WP_MAX_POPULATION = UInt64(1_000_000)

mutable struct Engine
    handle::Ptr{Cvoid}
    library::String
    closed::Bool
end

function Engine(library::AbstractString, population::UInt64, max_population::UInt64, seed::UInt64)
    max_population == 0 && error("max_population must be positive")
    max_population > WP_MAX_POPULATION && error("max_population exceeds native safety limit")
    population > max_population && error("population exceeds configured maximum")

    lib = String(library)
    handle_ref = Ref{Ptr{Cvoid}}(C_NULL)
    status = ccall((:wp_engine_create, lib), UInt32,
        (UInt64, UInt64, UInt64, Ref{Ptr{Cvoid}}),
        population, max_population, seed, handle_ref)
    status == WP_OK || error("wp_engine_create failed with code $status")
    handle_ref[] == C_NULL && error("native engine returned NULL")

    version = ccall((:wp_engine_abi_version, lib), UInt32, ())
    if version != ABI_VERSION
        ccall((:wp_engine_destroy, lib), Cvoid, (Ptr{Cvoid},), handle_ref[])
        error("unsupported WarPigs ABI version $version")
    end

    engine = Engine(handle_ref[], lib, false)
    finalizer(close, engine)
    return engine
end

function close(engine::Engine)
    engine.closed && return nothing
    engine.handle == C_NULL && (engine.closed = true; return nothing)
    ccall((:wp_engine_destroy, engine.library), Cvoid, (Ptr{Cvoid},), engine.handle)
    engine.handle = C_NULL
    engine.closed = true
    return nothing
end

function _check_open(engine::Engine)
    engine.closed && error("WarPigs engine is closed")
    engine.handle == C_NULL && error("WarPigs engine has no native handle")
end

function population(engine::Engine)
    _check_open(engine)
    return ccall((:wp_engine_population, engine.library), UInt64, (Ptr{Cvoid},), engine.handle)
end

function tick(engine::Engine)
    _check_open(engine)
    return ccall((:wp_engine_tick, engine.library), UInt64, (Ptr{Cvoid},), engine.handle)
end

function configuration_count(engine::Engine)
    _check_open(engine)
    return ccall((:wp_engine_configuration_count, engine.library), UInt32, ())
end

function configuration(engine::Engine, index::UInt64)
    _check_open(engine)
    buffer = zeros(UInt8, 5)
    status = ccall((:wp_engine_configuration_text, engine.library), UInt32,
        (Ptr{Cvoid}, UInt64, Ptr{UInt8}, Csize_t),
        engine.handle, index, pointer(buffer), length(buffer))
    status == WP_OK || error("configuration lookup failed with code $status")
    return String(buffer[1:4])
end

function step!(engine::Engine, action::UInt32)
    _check_open(engine)
    action > 3 && error("invalid action")
    status = ccall((:wp_engine_step, engine.library), UInt32,
        (Ptr{Cvoid}, UInt32), engine.handle, action)
    status == WP_OK || error("step failed with code $status")
    return nothing
end

"""Collect the native 81-bin configuration histogram as a Julia vector."""
function configuration_histogram(engine::Engine)
    _check_open(engine)
    bins = zeros(UInt64, 81)
    status = ccall((:wp_engine_configuration_histogram, engine.library), UInt32,
        (Ptr{Cvoid}, Ptr{UInt64}, Csize_t), engine.handle, pointer(bins), length(bins))
    status == WP_OK || error("histogram failed with code $status")
    return bins
end

end # module
