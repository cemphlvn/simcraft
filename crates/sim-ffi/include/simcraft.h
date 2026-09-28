/* simcraft C API — ABI version 1.
 *
 * One game.ron runs unchanged in every host. The host loads a whole game as text,
 * talks JSON for everything (same protocol as `simcraft-agent`, see docs/architecture.md),
 * and copies entities into a flat array each frame without JSON.
 *
 * Rules
 *  - Every function is NULL-safe and never lets a panic cross the boundary.
 *  - Every `char*` returned by this library is freed with simcraft_string_free.
 *  - One handle is used from one thread at a time (the engine uses its own worker threads).
 *  - If the ABI changes, SIMCRAFT_ABI_VERSION changes; check simcraft_abi_version() at startup.
 */
#ifndef SIMCRAFT_H
#define SIMCRAFT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SIMCRAFT_ABI_VERSION 1

typedef struct SimcraftSim SimcraftSim;

/* One entity for one frame. */
typedef struct SimcraftEntity {
    uint64_t id;
    int64_t  x;
    int64_t  y;
    uint32_t kind;   /* index; simcraft_kind_name() gives the name (alphabetical, as in info.kinds) */
    uint32_t glyph;  /* Unicode code point of the current state's glyph: the designer's state -> look map */
} SimcraftEntity;

uint32_t simcraft_abi_version(void);

/* Load from text. NULL on failure; then *out_error (if out_error != NULL) holds
 * {"ok":false,"stage":"load|validate","errors":[...]} — free it. */
SimcraftSim *simcraft_new(const char *game_ron, const char *engine_toml, char **out_error);
void simcraft_free(SimcraftSim *sim);
void simcraft_string_free(char *s);

/* One JSON request -> one JSON response (info, observe, act, step, hash, snapshot, restore). */
char *simcraft_request(SimcraftSim *sim, const char *request_json);

/* Advance n ticks (stops at the end / max_ticks). Returns the tick, -1 if sim is NULL. */
int64_t simcraft_step(SimcraftSim *sim, uint32_t n);

/* Copy up to cap entities (id order) into out; returns the total. out may be NULL (count only). */
size_t simcraft_entities(const SimcraftSim *sim, SimcraftEntity *out, size_t cap);

/* Kind index -> name, NULL if out of range. Owned by the handle: do not free. */
const char *simcraft_kind_name(const SimcraftSim *sim, uint32_t kind);

/* Every bus message since the last drain, as a JSON array (start, act, event, tick, end, restore). */
char *simcraft_drain(SimcraftSim *sim);

#ifdef __cplusplus
}
#endif

#endif /* SIMCRAFT_H */
