#include "warpigs.h"

#include <array>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <chrono>

int main() {
    constexpr std::uint64_t population = 100'000;
    wp_engine_t *engine = nullptr;
    const auto begin = std::chrono::steady_clock::now();
    assert(wp_engine_create(population, population, 0xA11CEULL, &engine) == WP_OK);
    const auto created = std::chrono::steady_clock::now();
    assert(engine != nullptr);
    assert(wp_engine_population(engine) == population);

    assert(wp_engine_step(engine, WP_PREPARE) == WP_OK);
    assert(wp_engine_step(engine, WP_START) == WP_OK);
    assert(wp_engine_step(engine, WP_QUARANTINE) == WP_OK);
    assert(wp_engine_step(engine, WP_TERMINATE) == WP_OK);

    std::array<std::uint64_t, 81> bins{};
    assert(wp_engine_configuration_histogram(engine, bins.data(), bins.size()) == WP_OK);
    std::uint64_t total = 0;
    for (const auto count : bins) total += count;
    assert(total == population);
    assert(wp_engine_population(engine) == population);
    assert(wp_engine_tick(engine) == 4);

    const auto end = std::chrono::steady_clock::now();
    const auto create_ms = std::chrono::duration_cast<std::chrono::milliseconds>(created - begin).count();
    const auto total_ms = std::chrono::duration_cast<std::chrono::milliseconds>(end - begin).count();
    std::printf("warpigs stress: population=%llu create_ms=%lld total_ms=%lld\n",
                static_cast<unsigned long long>(population),
                static_cast<long long>(create_ms),
                static_cast<long long>(total_ms));

    wp_engine_destroy(engine);
    return 0;
}
