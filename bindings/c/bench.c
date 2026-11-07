/* The binding benchmark's C rows (docs/02-research/benches/s4-bindings.md).
 *   cc -O2 -std=c11 bindings/c/bench.c -Ibindings/c -Ltarget/release -lteigeodata_ffi \
 *      -Wl,-rpath,target/release -o target/c-bench && target/c-bench
 */
#include <stdio.h>
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
    return 0;
}
