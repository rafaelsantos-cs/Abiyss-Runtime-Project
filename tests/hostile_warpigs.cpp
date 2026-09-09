#include "warpigs.h"

#include <cassert>
#include <cstdint>
#include <random>

int main() {
    // Hostile-but-safe sequence fuzzing: invalid actions and legal transitions
    // are mixed to verify that failures never mutate state.
    std::mt19937_64 rng(0xC0FFEEULL);
    std::uniform_int_distribution<std::uint32_t> action_dist(0, 255);

    for (unsigned scenario = 0; scenario < 250; ++scenario) {
        wp_engine_t *engine = nullptr;
        assert(wp_engine_create(32, 32, scenario + 1, &engine) == WP_OK);
        assert(engine != nullptr);

        std::uint64_t tick = 0;
        std::uint32_t expected_state = WP_CREATED;
        for (unsigned round = 0; round < 200; ++round) {
            const auto action = action_dist(rng);
            const auto before_tick = wp_engine_tick(engine);
            const auto before_population = wp_engine_population(engine);
            const auto result = wp_engine_step(engine, action);

            assert(before_tick == tick);
            assert(before_population == 32);
            assert(wp_engine_population(engine) == 32);

            bool legal = false;
            std::uint32_t target = expected_state;
            if (expected_state == WP_CREATED && action == WP_PREPARE) {
                legal = true;
                target = WP_READY;
            } else if (expected_state == WP_READY && (action == WP_START || action == WP_QUARANTINE || action == WP_TERMINATE)) {
                legal = true;
                target = (action == WP_START) ? WP_RUNNING :
                         (action == WP_QUARANTINE) ? WP_QUARANTINED : WP_TERMINATED;
            } else if (expected_state == WP_RUNNING && (action == WP_QUARANTINE || action == WP_TERMINATE)) {
                legal = true;
                target = (action == WP_QUARANTINE) ? WP_QUARANTINED : WP_TERMINATED;
            } else if (expected_state == WP_QUARANTINED && action == WP_TERMINATE) {
                legal = true;
                target = WP_TERMINATED;
            }

            if (legal) {
                assert(result == WP_OK);
                ++tick;
                expected_state = target;
                assert(wp_engine_tick(engine) == tick);
            } else {
                assert(result == (action > WP_TERMINATE ? WP_ERR_INVALID_ARGUMENT : WP_ERR_STATE));
                assert(wp_engine_tick(engine) == tick);
            }
        }

        wp_engine_destroy(engine);
    }

    // Null-only calls are valid defensive ABI probes.
    assert(wp_engine_abi_version() == 1U);
    assert(wp_engine_configuration_count() == 81U);
    assert(wp_engine_population(nullptr) == 0);
    assert(wp_engine_tick(nullptr) == 0);
    assert(wp_engine_step(nullptr, WP_START) == WP_ERR_NULL);
    wp_engine_destroy(nullptr);

    return 0;
}
