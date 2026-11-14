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

/* Row `row` of the string column `name` (UTF-8 or a dictionary) of a v1 blob, or NULL. */
static const char *column_str(tg_bytes blob, const char *name, uint32_t row, uint32_t *len) {
    const uint8_t *b = blob.ptr;
    uint32_t rows, columns;
    memcpy(&rows, b + 4, 4);
    memcpy(&columns, b + 8, 4);
    for (uint32_t i = 0; i < columns && row < rows; i++) {
        const uint8_t *d = b + 16 + 40 * i;
        if (strncmp((const char *)d, name, 16) != 0) continue;
        uint32_t f[6];
        memcpy(f, d + 16, sizeof f); /* type, data, data length, aux, aux length, nulls */
        uint32_t at, end;
        const uint8_t *strings;
        if (f[0] == 7) { /* UTF-8: rows + 1 offsets into the aux bytes */
            memcpy(&at, b + f[1] + 4 * row, 4);
            memcpy(&end, b + f[1] + 4 * (row + 1), 4);
            strings = b + f[3];
        } else if (f[0] == 11 || f[0] == 12) { /* a dictionary: a code per row */
            uint32_t code = 0, count;
            if (f[0] == 11) {
                uint16_t c16;
                memcpy(&c16, b + f[1] + 2 * row, 2);
                code = c16;
            } else {
                memcpy(&code, b + f[1] + 4 * row, 4);
            }
            memcpy(&count, b + f[3], 4);
            memcpy(&at, b + f[3] + 4 + 4 * code, 4);
            memcpy(&end, b + f[3] + 4 + 4 * (code + 1), 4);
            strings = b + f[3] + 4 + 4 * (count + 1);
        } else {
            return NULL;
        }
        *len = end - at;
        return (const char *)strings + at;
    }
    return NULL;
}

static uint32_t blob_rows(tg_bytes blob) {
    uint32_t rows;
    memcpy(&rows, blob.ptr + 4, 4);
    return rows;
}

static bool str_is(tg_bytes blob, const char *column, uint32_t row, const char *want) {
    uint32_t len = 0;
    const char *got = column_str(blob, column, row, &len);
    return got != NULL && len == strlen(want) && memcmp(got, want, len) == 0;
}

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

    /* The engine handle on the fixture pack (bindings/parity.json, "engine"). */
    tg_engine *engine = NULL;
    CHECK(tg_engine_new(&engine) == TG_OK && engine != NULL);
    uint32_t layers = 0;
    const char *path = "bindings/fixtures/synth.tgd";
    CHECK(tg_engine_open_file(engine, (const uint8_t *)path, strlen(path), &layers) == TG_OK);
    CHECK(layers == 1);
    const char *id = "tgd:np/18n8a6";
    CHECK(tg_engine_get(engine, (const uint8_t *)id, strlen(id), &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "name", 0, "Synthland"));
    CHECK(str_is(out, "kind", 0, "country"));
    tg_bytes_free(out);
    const char *query = "Synthland";
    CHECK(tg_engine_search(engine, (const uint8_t *)query, strlen(query), 3, &out) == TG_OK);
    CHECK(str_is(out, "id", 0, id));
    tg_bytes_free(out);
    CHECK(tg_engine_reverse(engine, 27.5, 85.5, 3, &out) == TG_OK);
    CHECK(blob_rows(out) == 3 && str_is(out, "id", 0, "tgd:np/sx2ze9"));
    tg_bytes_free(out);
    CHECK(tg_engine_admin_at(engine, 27.5, 85.5, &out) == TG_OK);
    CHECK(blob_rows(out) == 2 && str_is(out, "id", 1, "tgd:np/avwsr1"));
    tg_bytes_free(out);
    const char *bad_id = "not an id";
    CHECK(tg_engine_get(engine, (const uint8_t *)bad_id, strlen(bad_id), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_get(NULL, (const uint8_t *)id, strlen(id), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);
    const char *nowhere = "/nonexistent.tgd";
    CHECK(tg_engine_open_file(engine, (const uint8_t *)nowhere, strlen(nowhere), &layers) == TG_ERROR_IO);
    CHECK(tg_last_error(NULL) == TG_ERROR_IO);
    tg_engine_free(engine);
    tg_engine_free(NULL);

    if (failures == 0) puts("c parity ok");
    return failures != 0;
}
