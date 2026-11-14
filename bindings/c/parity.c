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
    CHECK(layers == 0);
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
    const char *child = "tgd:np/avwsr1";
    CHECK(tg_engine_parent(engine, (const uint8_t *)child, strlen(child), &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "id", 0, id));
    tg_bytes_free(out);
    CHECK(tg_engine_children(engine, (const uint8_t *)id, strlen(id), &out) == TG_OK);
    CHECK(blob_rows(out) == 8 && str_is(out, "id", 3, child));
    tg_bytes_free(out);
    CHECK(tg_engine_within_radius(engine, 27.5, 85.5, 20000.0, 10, &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "id", 0, "tgd:np/sx2ze9"));
    tg_bytes_free(out);
    CHECK(tg_engine_within_radius(engine, 27.5, 85.5, -1.0, 10, &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_tz_at(engine, 27.5, 85.5, &out) == TG_OK && str_is(out, "tz", 0, "Zone 1"));
    tg_bytes_free(out);
    CHECK(tg_engine_by_iso(engine, (const uint8_t *)"np", 2, &out) == TG_OK && str_is(out, "id", 0, id));
    tg_bytes_free(out);
    const char *lalek = "Lalek";
    tg_search_options focus = {.has_focus_lat = true, .focus_lat = 26.80005, .has_focus_lon = true, .focus_lon = 87.087849, .has_focus_scale_m = true, .focus_scale_m = 1000.0};
    CHECK(tg_engine_search_with(engine, (const uint8_t *)lalek, strlen(lalek), 3, &focus, &out) == TG_OK);
    CHECK(blob_rows(out) == 3 && str_is(out, "id", 1, "tgd:np/tv88y0"));
    tg_bytes_free(out);
    CHECK(tg_engine_search_with(engine, (const uint8_t *)lalek, strlen(lalek), 3, NULL, &out) == TG_OK);
    CHECK(blob_rows(out) == 3 && !str_is(out, "id", 1, "tgd:np/tv88y0"));
    tg_bytes_free(out);
    tg_search_options half = {.has_focus_lat = true, .focus_lat = 1.0};
    CHECK(tg_engine_search_with(engine, (const uint8_t *)lalek, strlen(lalek), 3, &half, &out) == TG_ERROR_INVALID_ARGUMENT);
    /* Open options: the byte at 173392 (parity.json, open_with) lies inside a section. */
    FILE *f = fopen(path, "rb");
    CHECK(f != NULL);
    static uint8_t pack[1 << 20];
    size_t pack_len = fread(pack, 1, sizeof pack, f);
    fclose(f);
    CHECK(pack_len > 173392 && pack_len < sizeof pack);
    pack[173392] ^= 0xff;
    tg_engine *scratch = NULL;
    CHECK(tg_engine_new(&scratch) == TG_OK);
    tg_open_options eager = {.verify = TG_VERIFY_EAGER};
    CHECK(tg_engine_open_bytes_with(scratch, pack, pack_len, &eager, &layers) == TG_ERROR_CORRUPT);
    CHECK(tg_engine_open_bytes_with(scratch, pack, pack_len, NULL, &layers) == TG_OK && layers == 0);
    tg_open_options every = {.priority = 2, .verify = TG_VERIFY_EAGER, .load = TG_LOAD_READ, .has_outline_level = true, .outline_level = 1};
    CHECK(tg_engine_open_file_with(scratch, (const uint8_t *)path, strlen(path), &every, &layers) == TG_OK && layers == 1);
    tg_open_options odd = {.verify = 99};
    CHECK(tg_engine_open_file_with(scratch, (const uint8_t *)path, strlen(path), &odd, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_open_options deep = {.has_outline_level = true, .outline_level = 256};
    CHECK(tg_engine_open_file_with(scratch, (const uint8_t *)path, strlen(path), &deep, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(scratch);
    tg_engine *points = NULL;
    CHECK(tg_engine_new(&points) == TG_OK);
    const char *point = "point", *langs = "en, ne", *pointy = "pointy";
    tg_open_options only = {.components = (const uint8_t *)point, .components_len = strlen(point), .languages = (const uint8_t *)langs, .languages_len = strlen(langs)};
    CHECK(tg_engine_open_file_with(points, (const uint8_t *)path, strlen(path), &only, &layers) == TG_OK);
    CHECK(tg_engine_get(points, (const uint8_t *)id, strlen(id), &out) == TG_OK && blob_rows(out) == 1);
    tg_bytes_free(out);
    const char *synthland = "Synthland";
    CHECK(tg_engine_search(points, (const uint8_t *)synthland, strlen(synthland), 1, &out) == TG_ERROR_MISSING_COMPONENT);
    tg_open_options unknown = {.components = (const uint8_t *)pointy, .components_len = strlen(pointy)};
    CHECK(tg_engine_open_file_with(points, (const uint8_t *)path, strlen(path), &unknown, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_open_options broken = {.components = NULL, .components_len = 3};
    CHECK(tg_engine_open_file_with(points, (const uint8_t *)path, strlen(path), &broken, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(points);
    const char *postal = "106-01";
    CHECK(tg_engine_postal_lookup(engine, (const uint8_t *)postal, strlen(postal), (const uint8_t *)"np", 2, &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "postal", 0, "10601") && str_is(out, "id", 0, "tgd:np/vasbex"));
    tg_bytes_free(out);
    CHECK(tg_engine_postal_at(engine, 27.5, 85.5, &out) == TG_OK);
    CHECK(blob_rows(out) == 2 && str_is(out, "code_kind", 1, "part"));
    tg_bytes_free(out);
    const char *bad_id = "not an id";
    CHECK(tg_engine_get(engine, (const uint8_t *)bad_id, strlen(bad_id), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_get(NULL, (const uint8_t *)id, strlen(id), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);
    const char *nowhere = "/nonexistent.tgd";
    CHECK(tg_engine_open_file(engine, (const uint8_t *)nowhere, strlen(nowhere), &layers) == TG_ERROR_IO);
    CHECK(tg_last_error(NULL) == TG_ERROR_IO);
    const char *dem = "bindings/fixtures/dem.tgd";
    CHECK(tg_engine_open_file(engine, (const uint8_t *)dem, strlen(dem), &layers) == TG_OK && layers == 1);
    CHECK(tg_engine_elevation_at(engine, 27.84, 76.01, &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "source", 0, "copernicus-glo30"));
    tg_bytes_free(out);
    bool removed = false;
    CHECK(tg_engine_remove(engine, 1, &removed) == TG_OK && removed);
    CHECK(tg_engine_remove(engine, 1, &removed) == TG_OK && !removed);
    tg_engine_free(engine);
    tg_engine_free(NULL);

    /* The zone rules (parity.json, tz) on the rules-only fixture. */
    tg_engine *rules = NULL;
    CHECK(tg_engine_new(&rules) == TG_OK);
    const char *tz_path = "bindings/fixtures/tz.tgd", *ny = "America/New_York";
    uint32_t tz_layer = 0;
    CHECK(tg_engine_open_file(rules, (const uint8_t *)tz_path, strlen(tz_path), &tz_layer) == TG_OK);
    tg_bytes version = {0};
    CHECK(tg_engine_tz_version(rules, &version) == TG_OK && version.len == 5 && memcmp(version.ptr, "2026f", 5) == 0);
    tg_bytes_free(version);
    CHECK(tg_engine_tz_offset_at(rules, (const uint8_t *)ny, strlen(ny), 1719835200, &out) == TG_OK && blob_rows(out) == 1 && str_is(out, "abbr", 0, "EDT"));
    tg_bytes_free(out);
    CHECK(tg_engine_tz_local(rules, (const uint8_t *)ny, strlen(ny), 2024, 3, 10, 2, 30, 0, &out) == TG_OK && str_is(out, "kind", 0, "gap"));
    tg_bytes_free(out);
    CHECK(tg_engine_tz_local(rules, (const uint8_t *)ny, strlen(ny), 2024, 13, 1, 0, 0, 0, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(rules);

#if TEIGEODATA_FEATURE_STORE
    /* The store (parity.json, store): the fixture root, a mirror that refuses connections.
     * A lite library's header leaves it out. */
    FILE *rf = fopen("bindings/fixtures/store-root.json", "rb");
    CHECK(rf != NULL);
    static uint8_t root[1 << 16];
    size_t root_len = fread(root, 1, sizeof root, rf);
    fclose(rf);
    const char *mirror = "http://127.0.0.1:1/", *missing = "target/c-store-missing";
    const char *bad_dir = "target/c-store-bad", *dir = "target/c-store", *np = "np";
    tg_store *store = NULL;
    CHECK(tg_store_open((const uint8_t *)missing, strlen(missing), &store) == TG_ERROR_REFUSED);
    CHECK(tg_store_create((const uint8_t *)bad_dir, strlen(bad_dir), (const uint8_t *)mirror, strlen(mirror), (const uint8_t *)mirror, strlen(mirror), (const uint8_t *)"{}", 2, &store) == TG_ERROR_REFUSED);
    CHECK(tg_store_create((const uint8_t *)dir, strlen(dir), (const uint8_t *)mirror, strlen(mirror), (const uint8_t *)mirror, strlen(mirror), root, root_len, &store) == TG_OK);
    tg_store_free(store);
    CHECK(tg_store_open((const uint8_t *)dir, strlen(dir), &store) == TG_OK);
    CHECK(tg_store_damaged(store, &out) == TG_OK && out.len == 0);
    tg_bytes_free(out);
    CHECK(tg_store_catalog(store, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_status(store, false, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    tg_engine *installed = NULL;
    CHECK(tg_store_engine(store, &installed) == TG_OK);
    CHECK(tg_engine_layers(installed, &layers) == TG_OK && layers == 0);
    CHECK(tg_store_find(store, installed, (const uint8_t *)np, strlen(np), 3, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_find(store, installed, (const uint8_t *)np, strlen(np), 10001, &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_locate(store, installed, 27.7, 85.3, 1000, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_locate(store, installed, 91.0, 0.0, 1000, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(installed);
    tg_job *job = NULL;
    tg_install_options wrong = {.components = (const uint8_t *)pointy, .components_len = strlen(pointy)};
    CHECK(tg_store_install(store, (const uint8_t *)np, strlen(np), &wrong, &job) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_remove(store, (const uint8_t *)np, strlen(np), &out) == TG_ERROR_REFUSED);
    // A platform's pass (D-184) needs a downloads directory; with one, the catalog's refresh
    // fails on the refusing mirror.
    const char *dl = "target/c-store-dl";
    CHECK(tg_store_install_pass(store, (const uint8_t *)np, strlen(np), (const uint8_t *)"", 0, NULL, &job) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_install_pass(store, (const uint8_t *)np, strlen(np), (const uint8_t *)dl, strlen(dl), NULL, &job) == TG_OK);
    CHECK(tg_job_result(job, &out) == TG_ERROR_NETWORK);
    tg_job_free(job);
    CHECK(tg_store_refresh(store, (const uint8_t *)np, strlen(np), &job) == TG_OK);
    CHECK(tg_job_result(job, &out) == TG_ERROR_NETWORK);
    bool ended = false;
    CHECK(tg_job_is_ended(job, &ended) == TG_OK && ended);
    uint64_t seq = 0, next = 0, done = 0, total = 1;
    CHECK(tg_job_seq(job, &seq) == TG_OK && tg_job_wait_progress(job, seq, 60000, &next) == TG_OK && next == seq);
    CHECK(tg_job_done_bytes(job, &done) == TG_OK && tg_job_total_bytes(job, &total) == TG_OK && done == total);
    CHECK(tg_job_cancel(job) == TG_OK);
    tg_job_free(job);
    tg_store_free(store);
#endif

    if (failures == 0) puts("c parity ok");
    return failures != 0;
}
