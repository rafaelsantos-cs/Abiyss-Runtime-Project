#include "warpigs.h"

#include <algorithm>
#include <array>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <set>
#include <string>
#include <vector>

static std::string configuration(wp_engine_t *engine, std::uint64_t index) {
    char out[5] = {};
    assert(wp_engine_configuration_text(engine, index, out, sizeof(out)) == WP_OK);
    return std::string(out);
}

int main() {
    // Exact configuration-space checks.
    wp_engine_t *engine = nullptr;
    assert(wp_engine_create(1000, 10000, 0x12345678ULL, &engine) == WP_OK);
    assert(engine != nullptr);
    assert(wp_engine_population(engine) == 1000);
    assert(wp_engine_tick(engine) == 0);

    std::set<std::uint8_t> seen_codes;
    std::array<std::uint64_t, 81> histogram{};
    assert(wp_engine_configuration_histogram(engine, histogram.data(), histogram.size()) == WP_OK);
    std::uint64_t total = 0;
    for (std::size_t i = 0; i < histogram.size(); ++i) {
        total += histogram[i];
    }
    assert(total == 1000);

    // The C ABI must never require a hidden fixed output size.
    char too_small[4] = {};
    assert(wp_engine_configuration_text(engine, 0, too_small, sizeof(too_small)) == WP_ERR_BUFFER);
    assert(wp_engine_configuration_text(engine, 1000, too_small, sizeof(too_small)) == WP_ERR_BOUNDS);

    // Configuration text is always exactly four ternary digits.
    for (std::uint64_t i = 0; i < 100; ++i) {
        const auto text = configuration(engine, i);
        assert(text.size() == 4);
        assert(std::all_of(text.begin(), text.end(), [](char ch) { return ch >= '0' && ch <= '2'; }));
        std::uint8_t code = 0;
        assert(wp_engine_configuration_code(engine, i, &code) == WP_OK);
        assert(code < 81);
        seen_codes.insert(code);
    }
    assert(!seen_codes.empty());

    // Batch state changes are all-or-nothing at the engine level.
    assert(wp_engine_step(engine, WP_PREPARE) == WP_OK);
    assert(wp_engine_tick(engine) == 1);
    std::uint64_t created = 0, ready = 0, running = 0, quarantined = 0, terminated = 0;
    assert(wp_engine_state_counts(engine, &created, &ready, &running, &quarantined, &terminated) == WP_OK);
    assert(created == 0 && ready == 1000 && running == 0 && quarantined == 0 && terminated == 0);

    assert(wp_engine_step(engine, WP_START) == WP_OK);
    assert(wp_engine_tick(engine) == 2);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, &running, nullptr, nullptr) == WP_OK);
    assert(running == 1000);

    assert(wp_engine_step(engine, WP_QUARANTINE) == WP_OK);
    assert(wp_engine_tick(engine) == 3);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, nullptr, &quarantined, nullptr) == WP_OK);
    assert(quarantined == 1000);

    assert(wp_engine_step(engine, WP_TERMINATE) == WP_OK);
    assert(wp_engine_tick(engine) == 4);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, nullptr, nullptr, &terminated) == WP_OK);
    assert(terminated == 1000);

    // A terminated population rejects further transitions without changing tick.
    assert(wp_engine_step(engine, WP_START) == WP_ERR_STATE);
    assert(wp_engine_tick(engine) == 4);

    // Determinism: identical seed and population produce identical configurations.
    wp_engine_t *a = nullptr;
    wp_engine_t *b = nullptr;
    assert(wp_engine_create(128, 128, 42, &a) == WP_OK);
    assert(wp_engine_create(128, 128, 42, &b) == WP_OK);
    for (std::uint64_t i = 0; i < 128; ++i) {
        assert(configuration(a, i) == configuration(b, i));
        char aid[64] = {};
        char bid[64] = {};
        assert(wp_engine_identity(a, i, aid, sizeof(aid)) == WP_OK);
        assert(wp_engine_identity(b, i, bid, sizeof(bid)) == WP_OK);
        // Identity is telemetry data and may intentionally differ.
        (void)aid;
        (void)bid;
    }

    wp_engine_destroy(b);
    wp_engine_destroy(a);
    wp_engine_destroy(engine);
    std::puts("warpigs native tests: PASS");
    return 0;
}
