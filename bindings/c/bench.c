/* The binding benchmark's C rows (docs/02-research/benches/s4-bindings.md). With
 * TEIGEODATA_BENCH_PACK naming a pack, the fifth case runs the 1,000-row query on it.
 *   cc -O2 -std=c11 bindings/c/bench.c -Ibindings/c -Ltarget/release -lteigeodata_ffi \
 *      -Wl,-rpath,target/release -o target/c-bench && target/c-bench
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "teigeodata.h"

static double now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1e9 + (double)t.tv_nsec;
}

#define BEST(n, body)                                       \
    ({                                                      \
        double out_ = 1e300;                                \
        for (int r_ = 0; r_ < 5; r_++) {                    \
            double t_ = now_ns();                           \
            for (long i_ = 0; i_ < (n); i_++) { body; }     \
            double ns_ = (now_ns() - t_) / (double)(n);     \
            if (ns_ < out_) out_ = ns_;                     \
        }                                                   \
        out_;                                               \
    })

static bool yes(void *ctx, uint32_t s) {
    (void)ctx;
    (void)s;
    return true;
}

static double lat_sum(const uint8_t *b) {
    uint32_t at, len;
    memcpy(&at, b + 76, 4);
    memcpy(&len, b + 80, 4);
    double s = 0;
    for (uint32_t i = 0; i < len; i += 8) {
        double v;
        memcpy(&v, b + at + i, 8);
        s += v;
    }
    return s;
}

/* The descriptor of column `name`: type, data, data length, aux, aux length, nulls. */
static const uint8_t *column(const uint8_t *b, const char *name) {
    uint32_t columns;
    memcpy(&columns, b + 8, 4);
    for (uint32_t i = 0; i < columns; i++)
        if (strncmp((const char *)b + 16 + 40 * i, name, 16) == 0) return b + 16 + 40 * i + 16;
    return NULL;
}

static uint32_t u32(const uint8_t *p) {
    uint32_t v;
    memcpy(&v, p, 4);
    return v;
}

/* The 1,000-row query's decoding: every `name` (offsets into UTF-8) and the sum of `lat`. */
static double decode(const uint8_t *b) {
    const uint8_t *names = column(b, "name"), *lat = column(b, "lat");
    if (names == NULL || lat == NULL || u32(names) != 7 || u32(lat) != 6) return -1;
    /* A C string is a pointer and a length into the blob: nothing to copy. */
    uint32_t rows = u32(b + 4), offsets = u32(names + 4);
    double total = 0;
    for (uint32_t r = 0; r < rows; r++)
        total += (double)(u32(b + offsets + 4 * (r + 1)) - u32(b + offsets + 4 * r));
    uint32_t at = u32(lat + 4);
    for (uint32_t r = 0; r < rows; r++) {
        double v;
        memcpy(&v, b + at + 8 * r, 8);
        total += v;
    }
    return total;
}

int main(void) {
    volatile double sink = 0;
    const char *k = "Kathmandu";
    printf("%-10s %-22s %10.1f ns\n", "c", "noop", BEST(10000000, sink += tg_noop()));
    printf("%-10s %-22s %10.1f ns\n", "c", "fold", BEST(1000000, {
        tg_bytes o = {0};
        tg_fold((const uint8_t *)k, 9, &o);
        sink += (double)o.len;
        tg_bytes_free(o);
    }));
    printf("%-10s %-22s %10.1f ns\n", "c", "sample_rows(1000)+sum", BEST(10000, {
        tg_bytes o = {0};
        tg_sample_rows(1000, &o);
        sink += lat_sum(o.ptr);
        tg_bytes_free(o);
    }));
    printf("%-10s %-22s %10.1f ns\n", "c", "callback", BEST(1000, {
        uint32_t n = 0;
        tg_progress(1000, yes, NULL, &n);
        sink += n;
    }) / 1000.0);
    const char *pack = getenv("TEIGEODATA_BENCH_PACK");
    tg_engine *engine = NULL;
    uint32_t layer = 0;
    if (pack != NULL && tg_engine_new(&engine) == TG_OK &&
        tg_engine_open_file(engine, (const uint8_t *)pack, strlen(pack), &layer) == TG_OK) {
        tg_bytes o = {0};
        tg_engine_within_radius(engine, 27.7, 85.3, 50000.0, 1000, &o);
        uint32_t rows = o.len >= 8 ? u32(o.ptr + 4) : 0;
        tg_bytes_free(o);
        if (rows == 1000)
            printf("%-10s %-22s %10.1f ns\n", "c", "radius(1000)+decode", BEST(200, {
                tg_bytes q = {0};
                tg_engine_within_radius(engine, 27.7, 85.3, 50000.0, 1000, &q);
                sink += decode(q.ptr);
                tg_bytes_free(q);
            }));
        else
            printf("c: %s: the query does not return 1,000 rows\n", pack);
    }
    tg_engine_free(engine);
    return 0;
}
