#include "warpigs.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <set>
#include <string>
#include <thread>
#include <vector>

static std::string configuration(const wp_engine_t *engine, std::uint64_t index) {
    char out[5] = {};
    assert(wp_engine_configuration_text(engine, index, out, sizeof(out)) == WP_OK);
    return std::string(out);
}

static std::string code_text(std::uint8_t code) {
    char out[5] = {};
    assert(wp_configuration_code_text(code, out, sizeof(out)) == WP_OK);
    return std::string(out);
}

static void test_invalid_inputs() {
    wp_engine_t *engine = nullptr;
    assert(wp_engine_create(0, 100, 1, &engine) == WP_OK);
    assert(engine != nullptr);
    assert(wp_engine_population(engine) == 0);
    assert(wp_engine_step(engine, 99U) == WP_ERR_INVALID_ARGUMENT);
    assert(wp_engine_configuration_code(engine, 0, nullptr) == WP_ERR_NULL);
    char tiny[4] = {};
    assert(wp_engine_configuration_text(engine, 0, tiny, sizeof(tiny)) == WP_ERR_BUFFER);
    assert(wp_engine_configuration_text(engine, 0, tiny, 5) == WP_ERR_BOUNDS);
    assert(wp_engine_component_counts(engine, 0, nullptr, nullptr, nullptr) == WP_ERR_NULL);
    assert(wp_configuration_code_text(81, tiny, sizeof(tiny)) == WP_ERR_INVALID_ARGUMENT);
    assert(wp_configuration_code_text(0, nullptr, 5) == WP_ERR_NULL);
    wp_engine_destroy(engine);

    assert(wp_engine_create(1001, 1000, 1, &engine) == WP_ERR_LIMIT);
    assert(engine == nullptr);
    assert(wp_engine_create(1, 1'000'001, 1, &engine) == WP_ERR_LIMIT);
    assert(engine == nullptr);
}

static void test_all_81_codes() {
    std::set<std::string> decoded;
    for (std::uint8_t code = 0; code < 81; ++code) {
        const auto text = code_text(code);
        assert(text.size() == 4);
        assert(std::all_of(text.begin(), text.end(), [](char c) { return c >= '0' && c <= '2'; }));
        decoded.insert(text);
    }
    assert(decoded.size() == 81);
    assert(code_text(0) == "0000");
    assert(code_text(1) == "0001");
    assert(code_text(3) == "0010");
    assert(code_text(80) == "2222");
}

int main() {
    test_invalid_inputs();
    test_all_81_codes();

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

    // Every instance has exactly four components and every component is ternary.
    for (std::uint64_t i = 0; i < 100; ++i) {
        const auto text = configuration(engine, i);
        assert(text.size() == 4);
        assert(std::all_of(text.begin(), text.end(), [](char ch) { return ch >= '0' && ch <= '2'; }));
        std::uint8_t masked = 0, active = 0, uterus = 0;
        assert(wp_engine_component_counts(engine, i, &masked, &active, &uterus) == WP_OK);
        assert(static_cast<unsigned>(masked) + static_cast<unsigned>(active) + static_cast<unsigned>(uterus) == 4U);
        std::uint8_t code = 0;
        assert(wp_engine_configuration_code(engine, i, &code) == WP_OK);
        assert(code < 81);
    }

    std::set<std::string> identities;
    for (std::uint64_t i = 0; i < 100; ++i) {
        char id[64] = {};
        assert(wp_engine_identity(engine, i, id, sizeof(id)) == WP_OK);
        assert(std::strlen(id) > 0);
        identities.emplace(id);
    }
    assert(identities.size() == 100);

    assert(wp_engine_step(engine, WP_PREPARE) == WP_OK);
    assert(wp_engine_tick(engine) == 1);
    std::uint64_t created = 0, ready = 0, running = 0, quarantined = 0, terminated = 0;
    assert(wp_engine_state_counts(engine, &created, &ready, &running, &quarantined, &terminated) == WP_OK);
    assert(created == 0 && ready == 1000 && running == 0 && quarantined == 0 && terminated == 0);

    assert(wp_engine_step(engine, WP_START) == WP_OK);
    assert(wp_engine_tick(engine) == 2);
    assert(wp_engine_state_counts(engine, nullptr, nullptr, &running, nullptr, nullptr) == WP_OK);
    assert(running == 1000);

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
                std::uint8_t masked = 0, active = 0, uterus = 0;
                if (wp_engine_component_counts(engine, round % 100, &masked, &active, &uterus) != WP_OK) {
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
    }

    wp_engine_destroy(b);
    wp_engine_destroy(a);
    wp_engine_destroy(engine);
    std::puts("warpigs native tests: PASS");
    return 0;
}
