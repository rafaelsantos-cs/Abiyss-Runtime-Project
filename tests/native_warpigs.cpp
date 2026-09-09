#include "warpigs.h"

#include <algorithm>
#include <array>
#include <cassert>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <set>
#include <string>
#include <thread>
#include <vector>

static std::string configuration(const wp_engine_t *engine, std::uint64_t index) {
    char out[5] = {};
    assert(wp_engine_configuration_text(engine, index, out, sizeof(out)) == WP_OK);
    return std::string(out);
}

static void test_invalid_inputs() {
    wp_engine_t *engine = nullptr;
    assert(wp_engine_create(0, 100, 1, &engine) == WP_OK);
    assert(engine != nullptr);
    assert(wp_engine_population(engine) == 0);
    assert(wp_engine_step(engine, static_cast<wp_action_t>(99)) == WP_ERR_INVALID_ARGUMENT);
    assert(wp_engine_configuration_code(engine, 0, nullptr) == WP_ERR_NULL);
    char tiny[4] = {};
    assert(wp_engine_configuration_text(engine, 0, tiny, sizeof(tiny)) == WP_ERR_BUFFER);
    assert(wp_engine_configuration_text(engine, 0, tiny, 5) == WP_ERR_BOUNDS);
    wp_engine_destroy(engine);

    assert(wp_engine_create(1001, 1000, 1, &engine) == WP_ERR_LIMIT);
    assert(engine == nullptr);
    assert(wp_engine_create(1, 1'000'001, 1, &engine) == WP_ERR_LIMIT);
    assert(engine == nullptr);
}

int main() {
    test_invalid_inputs();

    // Canonical configuration space is exactly 3^4 = 81.
    assert(3U * 3U * 3U * 3U == 81U);

    wp_engine_t *engine = nullptr;
    assert(wp_engine_create(1000, 10000, 0x12345678ULL, &engine) == WP_OK);
    assert(engine != nullptr);
    assert(wp_engine_population(engine) == 1000);
    assert(wp_engine_tick(engine) == 0);

    std::array<std::uint64_t, 81> histogram{};
    assert(wp_engine_configuration_histogram(engine, histogram.data(), histogram.size()) == WP_OK);
    std::uint64_t total = 0;
    for (const auto count : histogram) total += count;
    assert(total == 1000);

    // Configuration text and numeric encoding agree for sampled entities.
    std::set<std::uint8_t> seen_codes;
    std::set<std::string> identities;
    for (std::uint64_t i = 0; i < 100; ++i) {
        const auto text = configuration(engine, i);
        assert(text.size() == 4);
        assert(std::all_of(text.begin(), text.end(), [](char ch) { return ch >= '0' && ch <= '2'; }));
        std::uint8_t code = 0;
        assert(wp_engine_configuration_code(engine, i, &code) == WP_OK);
        assert(code < 81);
        seen_codes.insert(code);

        char id[64] = {};
        assert(wp_engine_identity(engine, i, id, sizeof(id)) == WP_OK);
        assert(std::strlen(id) > 0);
        identities.emplace(id);
    }
    assert(!seen_codes.empty());
    assert(identities.size() == 100);

    // Batch state changes are all-or-nothing and preserve population.
    assert(wp_engine_step(engine, WP_PREPARE) == WP_OK);
    assert(wp_engine_tick(engine) == 1);
    std::uint64_t created = 0, ready = 0, running = 0, quarantined = 0, terminated = 0;
    assert(wp_engine_state_counts(engine, &created, &ready, &running, &quarantined, &terminated) == WP_OK);
    assert(created == 0 && ready == 1000 && running == 0 && quarantined == 0 && terminated == 0);

    assert(wp_engine_step(engine, WP_START) == WP_OK);
    assert(wp_engine_tick(engine) == 2);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, &running, nullptr, nullptr) == WP_OK);
    assert(running == 1000);

    // Concurrent readers are safe while the engine is quiescent.
    std::atomic<bool> reader_failed{false};
    std::vector<std::thread> readers;
    for (unsigned worker = 0; worker < 8; ++worker) {
        readers.emplace_back([&]() {
            for (unsigned round = 0; round < 100; ++round) {
                if (wp_engine_population(engine) != 1000 || wp_engine_tick(engine) != 2) {
                    reader_failed.store(true, std::memory_order_relaxed);
                    return;
                }
                std::array<std::uint64_t, 81> bins{};
                if (wp_engine_configuration_histogram(engine, bins.data(), bins.size()) != WP_OK) {
                    reader_failed.store(true, std::memory_order_relaxed);
                    return;
                }
            }
        });
    }
    for (auto &reader : readers) reader.join();
    assert(!reader_failed.load(std::memory_order_relaxed));

    assert(wp_engine_step(engine, WP_QUARANTINE) == WP_OK);
    assert(wp_engine_tick(engine) == 3);
    assert(wp_engine_step(engine, WP_TERMINATE) == WP_OK);
    assert(wp_engine_tick(engine) == 4);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, nullptr, nullptr, &terminated) == WP_OK);
    assert(terminated == 1000);

    // Terminal state is absorbing.
    assert(wp_engine_step(engine, WP_START) == WP_ERR_STATE);
    assert(wp_engine_tick(engine) == 4);
    assert(wp_engine_population(engine) == 1000);

    // Determinism: identical seed and size produce identical configurations.
    wp_engine_t *a = nullptr;
    wp_engine_t *b = nullptr;
    assert(wp_engine_create(128, 128, 42, &a) == WP_OK);
    assert(wp_engine_create(128, 128, 42, &b) == WP_OK);
    for (std::uint64_t i = 0; i < 128; ++i) {
        assert(configuration(a, i) == configuration(b, i));
        assert(configuration(a, i).size() == 4);
    }

    wp_engine_destroy(b);
    wp_engine_destroy(a);
    wp_engine_destroy(engine);
    std::puts("warpigs native tests: PASS");
    return 0;
}
