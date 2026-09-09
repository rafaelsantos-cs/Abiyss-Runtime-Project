#ifndef WARPIGS_H
#define WARPIGS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct wp_engine wp_engine_t;

typedef enum wp_component_state {
    WP_MASKED = 0,
    WP_ACTIVE = 1,
    WP_STERILE_UTERUS = 2
} wp_component_state_t;

typedef enum wp_lifecycle_state {
    WP_CREATED = 0,
    WP_READY = 1,
    WP_RUNNING = 2,
    WP_QUARANTINED = 3,
    WP_TERMINATED = 4
} wp_lifecycle_state_t;

typedef enum wp_action {
    WP_PREPARE = 0,
    WP_START = 1,
    WP_QUARANTINE = 2,
    WP_TERMINATE = 3
} wp_action_t;

typedef enum wp_error {
    WP_OK = 0,
    WP_ERR_NULL = 1,
    WP_ERR_INVALID_ARGUMENT = 2,
    WP_ERR_LIMIT = 3,
    WP_ERR_BOUNDS = 4,
    WP_ERR_STATE = 5,
    WP_ERR_BUFFER = 6,
    WP_ERR_INTERNAL = 255
} wp_error_t;

/* Create a fully in-memory sterile simulator. No host resources are touched. */
wp_error_t wp_engine_create(uint64_t population_size,
                            uint64_t max_population,
                            uint64_t seed,
                            wp_engine_t **out_engine);

void wp_engine_destroy(wp_engine_t *engine);

/* Advance one action over the complete population. The batch is prevalidated. */
wp_error_t wp_engine_step(wp_engine_t *engine, wp_action_t action);

uint64_t wp_engine_tick(const wp_engine_t *engine);
uint64_t wp_engine_population(const wp_engine_t *engine);

/* Return the base-3 configuration code (0..80) for one instance. */
wp_error_t wp_engine_configuration_code(const wp_engine_t *engine,
                                        uint64_t index,
                                        uint8_t *out_code);

/* Return the four component states as ASCII digits: 0000..2222. */
wp_error_t wp_engine_configuration_text(const wp_engine_t *engine,
                                       uint64_t index,
                                       char *out,
                                       size_t capacity);

wp_error_t wp_engine_lifecycle(const wp_engine_t *engine,
                               uint64_t index,
                               wp_lifecycle_state_t *out_state);

/* Copy a stable simulation-only identifier into caller storage. */
wp_error_t wp_engine_identity(const wp_engine_t *engine,
                              uint64_t index,
                              char *out,
                              size_t capacity);

/* Count lifecycle states. Output pointers may be NULL when a count is unused. */
wp_error_t wp_engine_state_counts(const wp_engine_t *engine,
                                  uint64_t *created,
                                  uint64_t *ready,
                                  uint64_t *running,
                                  uint64_t *quarantined,
                                  uint64_t *terminated);

/* Count configuration states. Caller supplies 81 slots. */
wp_error_t wp_engine_configuration_histogram(const wp_engine_t *engine,
                                             uint64_t *counts,
                                             size_t capacity);

#ifdef __cplusplus
}
#endif

#endif
