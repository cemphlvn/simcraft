/* cc -Iinclude examples/smoke.c ../../target/release/libsimcraft.a -o smoke && ./smoke ../../games/gamedev */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "simcraft.h"

static char *slurp(const char *dir, const char *file) {
    char path[1024];
    snprintf(path, sizeof path, "%s/%s", dir, file);
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); exit(1); }
    fseek(f, 0, SEEK_END); long n = ftell(f); rewind(f);
    char *s = malloc(n + 1); fread(s, 1, n, f); s[n] = 0; fclose(f);
    return s;
}

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : "../../games/wolf_sheep";
    if (simcraft_abi_version() != SIMCRAFT_ABI_VERSION) { puts("ABI mismatch"); return 1; }
    char *game = slurp(dir, "game.ron"), *panel = slurp(dir, "engine.toml"), *err = NULL;
    SimcraftSim *sim = simcraft_new(game, panel, &err);
    if (!sim) { printf("load failed: %s\n", err); simcraft_string_free(err); return 1; }

    printf("tick %lld\n", (long long)simcraft_step(sim, 24 * 7));
    size_t n = simcraft_entities(sim, NULL, 0);
    SimcraftEntity *es = calloc(n, sizeof *es);
    simcraft_entities(sim, es, n);
    for (size_t i = 0; i < n; i++)
        if (es[i].glyph != '#')
            printf("  %-8s id=%-3llu at (%lld,%lld) glyph '%c'\n", simcraft_kind_name(sim, es[i].kind),
                   (unsigned long long)es[i].id, (long long)es[i].x, (long long)es[i].y, (char)es[i].glyph);
    char *hash = simcraft_request(sim, "{\"cmd\":\"hash\"}");
    printf("%s\n", hash);
    simcraft_string_free(hash);
    free(es); simcraft_free(sim); free(game); free(panel);
    return 0;
}
