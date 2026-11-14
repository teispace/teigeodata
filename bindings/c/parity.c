/* The C ABI from C: compiles the generated header strictly (C11, and C++ with -x c++) and
 * calls every function. The full parity suite runs in the other languages and in
 * crates/teigeodata-ffi/tests; this proves the header matches the library a C caller links.
 *
 *   cc -std=c11 -Wall -Wextra -Werror -pedantic bindings/c/parity.c -Ibindings/c \
 *      -Ltarget/release -lteigeodata_ffi -o target/c-parity && target/c-parity
 */
#include <stdio.h>
#include <string.h>

#include "teigeodata.h"

static int failures = 0;
#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                \
        }                                                              \
    } while (0)

static bool upto3(void *ctx, uint32_t step) {
    *(uint32_t *)ctx += step;
    return step < 3;
}

int main(void) {
    tg_bytes out = {0};
    CHECK(tg_version(&out) == TG_OK && out.len == 5 && memcmp(out.ptr, "0.3.1", 5) == 0);
    tg_bytes_free(out);
    CHECK(tg_noop() == TG_OK);

    const char *text = "S\xc3\xa3o  Paulo";
    CHECK(tg_fold((const uint8_t *)text, strlen(text), &out) == TG_OK);
    CHECK(out.len == 9 && memcmp(out.ptr, "sao paulo", 9) == 0);
    tg_bytes_free(out);

    const uint8_t bad[] = {0xff};
    CHECK(tg_fold(bad, 1, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_bytes msg = {0};
    CHECK(tg_last_error(&msg) == TG_ERROR_INVALID_ARGUMENT && msg.len > 0);
    tg_bytes_free(msg);

    CHECK(tg_sample_rows(1000, &out) == TG_OK && out.len == 37120);
    CHECK(memcmp(out.ptr, "TGB1", 4) == 0 && ((uintptr_t)out.ptr % 8) == 0);
    tg_bytes_free(out);
    CHECK(tg_sample_rows(TG_SAMPLE_ROWS_MAX + 1, &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);

    uint32_t sum = 0, steps = 0;
    CHECK(tg_progress(2, upto3, &sum, &steps) == TG_OK && steps == 2 && sum == 3);
    CHECK(tg_progress(9, upto3, &sum, &steps) == TG_ERROR_CANCELLED && steps == 2);
    CHECK(tg_last_error(NULL) == TG_ERROR_CANCELLED);
    CHECK(tg_progress(1, NULL, NULL, &steps) == TG_ERROR_INVALID_ARGUMENT);
    tg_bytes_free((tg_bytes){0});

    if (failures == 0) puts("c parity ok");
    return failures != 0;
}
