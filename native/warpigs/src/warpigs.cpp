#include "warpigs.h"

#include <array>
#include <cstdint>
#include <random>
#include <string>
#include <mutex>
#include <vector>
#include <algorithm>

namespace {

constexpr std::size_t kPositions = 4;
constexpr std::uint64_t kAbsoluteMaxPopulation = 1'000'000;
constexpr std::size_t kIdentityBytes = 34; // "wp-" + 30 alphanumeric characters

struct Pig {
    std::array<std::uint8_t, kPositions> configuration{};
    wp_lifecycle_state_t lifecycle = WP_CREATED;
    std::string identity;
};

struct Engine {
    mutable std::mutex mutex;
    std::vector<Pig> pigs;
    std::uint64_t tick = 0;
    std::uint64_t max_population = 0;
};

bool valid_action(wp_action_t action) {
    return action >= WP_PREPARE && action <= WP_TERMINATE;
}

bool transition_allowed(wp_lifecycle_state_t state, wp_action_t action) {
    switch (state) {
    case WP_CREATED:    return action == WP_PREPARE;
    case WP_READY:      return action == WP_START || action == WP_QUARANTINE || action == WP_TERMINATE;
    case WP_RUNNING:    return action == WP_QUARANTINE || action == WP_TERMINATE;
    case WP_QUARANTINED:return action == WP_TERMINATE;
    case WP_TERMINATED: return false;
    }
    return false;
}

wp_lifecycle_state_t target_state(wp_action_t action) {
    switch (action) {
    case WP_PREPARE:    return WP_READY;
    case WP_START:      return WP_RUNNING;
    case WP_QUARANTINE: return WP_QUARANTINED;
    case WP_TERMINATE:  return WP_TERMINATED;
    }
    return WP_TERMINATED;
}

std::string make_identity(std::mt19937_64 &rng, std::size_t serial) {
    static constexpr char alphabet[] =
        "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    std::uniform_int_distribution<std::size_t> dist(0, sizeof(alphabet) - 2);
    std::string result = "wp-" + std::to_string(serial) + "-";
    while (result.size() < 33) {
        result.push_back(alphabet[dist(rng)]);
    }
    return result;
}

std::uint8_t encode_configuration(const std::array<std::uint8_t, kPositions> &states) {
    std::uint8_t code = 0;
    for (const auto state : states) {
        code = static_cast<std::uint8_t>(code * 3U + state);
    }
    return code;
}

bool valid_index(const Engine *engine, std::uint64_t index) {
    return engine != nullptr && index < engine->pigs.size();
}

} // namespace

struct wp_engine {
    Engine impl;
};

extern "C" {

wp_error_t wp_engine_create(std::uint64_t population_size,
                            std::uint64_t max_population,
                            std::uint64_t seed,
                            wp_engine_t **out_engine) {
    if (out_engine == nullptr) return WP_ERR_NULL;
    *out_engine = nullptr;
    if (max_population == 0 || max_population > kAbsoluteMaxPopulation) return WP_ERR_LIMIT;
    if (population_size > max_population) return WP_ERR_LIMIT;

    try {
        auto *engine = new wp_engine();
        engine->impl.max_population = max_population;
        std::mt19937_64 rng(seed);
        std::uniform_int_distribution<int> state_dist(0, 2);
        engine->impl.pigs.reserve(static_cast<std::size_t>(population_size));

        for (std::uint64_t i = 0; i < population_size; ++i) {
            Pig pig;
            for (auto &state : pig.configuration) {
                state = static_cast<std::uint8_t>(state_dist(rng));
            }
            pig.identity = make_identity(rng, static_cast<std::size_t>(i + 1));
            engine->impl.pigs.push_back(std::move(pig));
        }
        *out_engine = engine;
        return WP_OK;
    } catch (...) {
        return WP_ERR_INTERNAL;
    }
}

void wp_engine_destroy(wp_engine_t *engine) {
    delete engine;
}

wp_error_t wp_engine_step(wp_engine_t *engine, wp_action_t action) {
    if (engine == nullptr) return WP_ERR_NULL;
    if (!valid_action(action)) return WP_ERR_INVALID_ARGUMENT;

    std::lock_guard<std::mutex> guard(engine->impl.mutex);

    // Preflight the complete population. This makes the batch atomic with
    // respect to lifecycle validity: no partial batch transition is allowed.
    for (const auto &pig : engine->impl.pigs) {
        if (!transition_allowed(pig.lifecycle, action)) return WP_ERR_STATE;
    }

    const auto target = target_state(action);
    for (auto &pig : engine->impl.pigs) {
        pig.lifecycle = target;
    }
    ++engine->impl.tick;
    return WP_OK;
}

std::uint64_t wp_engine_tick(const wp_engine_t *engine) {
    if (engine == nullptr) return 0;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    return engine->impl.tick;
}

std::uint64_t wp_engine_population(const wp_engine_t *engine) {
    if (engine == nullptr) return 0;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    return static_cast<std::uint64_t>(engine->impl.pigs.size());
}

wp_error_t wp_engine_configuration_code(const wp_engine_t *engine,
                                        std::uint64_t index,
                                        std::uint8_t *out_code) {
    if (engine == nullptr || out_code == nullptr) return WP_ERR_NULL;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    if (!valid_index(&engine->impl, index)) return WP_ERR_BOUNDS;
    *out_code = encode_configuration(engine->impl.pigs[static_cast<std::size_t>(index)].configuration);
    return WP_OK;
}

wp_error_t wp_engine_configuration_text(const wp_engine_t *engine,
                                       std::uint64_t index,
                                       char *out,
                                       std::size_t capacity) {
    if (engine == nullptr || out == nullptr) return WP_ERR_NULL;
    if (capacity < 5) return WP_ERR_BUFFER;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    if (!valid_index(&engine->impl, index)) return WP_ERR_BOUNDS;
    const auto &config = engine->impl.pigs[static_cast<std::size_t>(index)].configuration;
    for (std::size_t i = 0; i < kPositions; ++i) {
        out[i] = static_cast<char>('0' + config[i]);
    }
    out[4] = '\0';
    return WP_OK;
}

wp_error_t wp_engine_lifecycle(const wp_engine_t *engine,
                               std::uint64_t index,
                               wp_lifecycle_state_t *out_state) {
    if (engine == nullptr || out_state == nullptr) return WP_ERR_NULL;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    if (!valid_index(&engine->impl, index)) return WP_ERR_BOUNDS;
    *out_state = engine->impl.pigs[static_cast<std::size_t>(index)].lifecycle;
    return WP_OK;
}

wp_error_t wp_engine_identity(const wp_engine_t *engine,
                              std::uint64_t index,
                              char *out,
                              std::size_t capacity) {
    if (engine == nullptr || out == nullptr) return WP_ERR_NULL;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    if (!valid_index(&engine->impl, index)) return WP_ERR_BOUNDS;
    const auto &id = engine->impl.pigs[static_cast<std::size_t>(index)].identity;
    if (capacity <= id.size()) return WP_ERR_BUFFER;
    std::copy(id.begin(), id.end(), out);
    out[id.size()] = '\0';
    return WP_OK;
}

wp_error_t wp_engine_state_counts(const wp_engine_t *engine,
                                  std::uint64_t *created,
                                  std::uint64_t *ready,
                                  std::uint64_t *running,
                                  std::uint64_t *quarantined,
                                  std::uint64_t *terminated) {
    if (engine == nullptr) return WP_ERR_NULL;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    std::uint64_t counts[5] = {0, 0, 0, 0, 0};
    for (const auto &pig : engine->impl.pigs) {
        const auto index = static_cast<unsigned>(pig.lifecycle);
        if (index > 4) return WP_ERR_INTERNAL;
        ++counts[index];
    }
    if (created) *created = counts[WP_CREATED];
    if (ready) *ready = counts[WP_READY];
    if (running) *running = counts[WP_RUNNING];
    if (quarantined) *quarantined = counts[WP_QUARANTINED];
    if (terminated) *terminated = counts[WP_TERMINATED];
    return WP_OK;
}

wp_error_t wp_engine_configuration_histogram(const wp_engine_t *engine,
                                             std::uint64_t *counts,
                                             std::size_t capacity) {
    if (engine == nullptr || counts == nullptr) return WP_ERR_NULL;
    if (capacity < 81) return WP_ERR_BUFFER;
    std::lock_guard<std::mutex> guard(engine->impl.mutex);
    std::fill(counts, counts + 81, 0);
    for (const auto &pig : engine->impl.pigs) {
        ++counts[encode_configuration(pig.configuration)];
    }
    return WP_OK;
}

} // extern "C"
