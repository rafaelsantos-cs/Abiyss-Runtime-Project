#ifndef WARPIGS_H
#define WARPIGS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct wp_engine wp_engine_t;

/* Fixed-width ABI constants. Do not expose language-specific enum layouts. */
#define WP_ABI_VERSION 1u
#define WP_MASKED 0u
#define WP_ACTIVE 1u
#define WP_STERILE_UTERUS 2u
#define WP_CREATED 0u
#define WP_READY 1u
#define WP_RUNNING 2u
#define WP_QUARANTINED 3u
#define WP_TERMINATED 4u
#define WP_PREPARE 0u
#define WP_START 1u
#define WP_QUARANTINE 2u
#define WP_TERMINATE 3u
#define WP_OK 0u
#define WP_ERR_NULL 1u
#define WP_ERR_INVALID_ARGUMENT 2u
#define WP_ERR_LIMIT 3u
#define WP_ERR_BOUNDS 4u
#define WP_ERR_STATE 5u
#define WP_ERR_BUFFER 6u
#define WP_ERR_INTERNAL 255u

uint32_t wp_engine_abi_version(void);
uint32_t wp_engine_configuration_count(void);

uint32_t wp_engine_create(uint64_t population_size,
                          uint64_t max_population,
                          uint64_t seed,
                          wp_engine_t **out_engine);

void wp_engine_destroy(wp_engine_t *engine);

/* Advance one action over the complete population. The batch is prevalidated. */
uint32_t wp_engine_step(wp_engine_t *engine, uint32_t action);

uint64_t wp_engine_tick(const wp_engine_t *engine);
uint64_t wp_engine_population(const wp_engine_t *engine);

uint32_t wp_engine_configuration_code(const wp_engine_t *engine,
                                      uint64_t index,
                                      uint8_t *out_code);

uint32_t wp_engine_configuration_text(const wp_engine_t *engine,
                                      uint64_t index,
                                      char *out,
                                      size_t capacity);

uint32_t wp_engine_lifecycle(const wp_engine_t *engine,
                             uint64_t index,
                             uint32_t *out_state);

uint32_t wp_engine_identity(const wp_engine_t *engine,
                            uint64_t index,
                            char *out,
                            size_t capacity);

/* Count this instance's four component roles. */
uint32_t wp_engine_component_counts(const wp_engine_t *engine,
                                    uint64_t index,
                                    uint8_t *masked,
                                    uint8_t *active,
                                    uint8_t *sterile_uterus);

uint32_t wp_engine_state_counts(const wp_engine_t *engine,
                                uint64_t *created,
                                uint64_t *ready,
                                uint64_t *running,
                                uint64_t *quarantined,
                                uint64_t *terminated);

uint32_t wp_engine_configuration_histogram(const wp_engine_t *engine,
                                           uint64_t *counts,
                                           size_t capacity);

#ifdef __cplusplus
}
#endif

#endif
