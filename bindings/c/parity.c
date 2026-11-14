/* The parity suite in C: every value of bindings/parity.json, read from the generated
 * parity_data.h, checked through the C ABI; and the ABI's own contract (NULL handles and
 * outputs, the last error, zeroed buffers). It compiles the header strictly (C11, and C++
 * with -x c++). Run it from the repository root:
 *
 *   cc -std=c11 -Wall -Wextra -Werror -pedantic bindings/c/parity.c -Ibindings/c \
 *      -Ltarget/release -lteigeodata_ffi -o target/c-parity && target/c-parity
 *
 * cargo xtask parity-mutants compiles a copy beside a planted parity_data.h, which must fail.
 */
#include <math.h> /* NAN only: no libm */
#include <stdio.h>
#include <string.h>

#include "parity_data.h"
#include "teigeodata.h"

static int failures = 0;
#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                \
        }                                                              \
    } while (0)
#define COUNT(a) (sizeof(a) / sizeof((a)[0]))
#define TEXT(s) (const uint8_t *)(s), strlen(s)
/* A fixture path: parity.json's paths are relative to bindings/. */
#define FIXTURE(rel) "bindings/" rel

/* The descriptor of column `name` of a v1 blob: type, data, data length, aux, aux length,
 * nulls; false when there is none. */
static bool column(tg_bytes blob, const char *name, uint32_t f[6]) {
    uint32_t columns;
    memcpy(&columns, blob.ptr + 8, 4);
    for (uint32_t i = 0; i < columns; i++) {
        const uint8_t *d = blob.ptr + 16 + 40 * i;
        if (strncmp((const char *)d, name, 16) == 0) {
            memcpy(f, d + 16, 6 * sizeof f[0]);
            return true;
        }
    }
    return false;
}

static uint32_t blob_rows(tg_bytes blob) {
    uint32_t rows;
    memcpy(&rows, blob.ptr + 4, 4);
    return rows;
}

/* Row `row` of the string column `name` (UTF-8 or a dictionary), or NULL. */
static const char *column_str(tg_bytes blob, const char *name, uint32_t row, uint32_t *len) {
    uint32_t f[6];
    if (row >= blob_rows(blob) || !column(blob, name, f)) return NULL;
    const uint8_t *b = blob.ptr;
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

/* Row `row` of the number column `name`, as a double; NAN when there is none. */
static double column_num(tg_bytes blob, const char *name, uint32_t row) {
    uint32_t f[6];
    if (row >= blob_rows(blob) || !column(blob, name, f)) return NAN;
    const uint8_t *at = blob.ptr + f[1];
    switch (f[0]) {
    case 1: { int32_t v; memcpy(&v, at + 4 * row, 4); return v; }
    case 2: { uint32_t v; memcpy(&v, at + 4 * row, 4); return v; }
    case 3: { int64_t v; memcpy(&v, at + 8 * row, 8); return (double)v; }
    case 4: { uint64_t v; memcpy(&v, at + 8 * row, 8); return (double)v; }
    case 5: { float v; memcpy(&v, at + 4 * row, 4); return v; }
    case 6: { double v; memcpy(&v, at + 8 * row, 8); return v; }
    case 8: return at[row];
    case 9: { int16_t v; memcpy(&v, at + 2 * row, 2); return v; }
    case 10: { uint16_t v; memcpy(&v, at + 2 * row, 2); return v; }
    default: return NAN;
    }
}

static bool str_is(tg_bytes blob, const char *name, uint32_t row, const char *want) {
    uint32_t len = 0;
    const char *got = column_str(blob, name, row, &len);
    return got != NULL && len == strlen(want) && memcmp(got, want, len) == 0;
}

/* The string column `name` is exactly `want`, row for row. */
static bool strs_are(tg_bytes blob, const char *name, const char *const *want, size_t count) {
    if (blob_rows(blob) != count) return false;
    for (size_t i = 0; i < count; i++) {
        if (!str_is(blob, name, (uint32_t)i, want[i])) return false;
    }
    return true;
}

/* The blob's column names are exactly `want`, in order. */
static bool columns_are(tg_bytes blob, const char *const *want, size_t count) {
    uint32_t columns;
    memcpy(&columns, blob.ptr + 8, 4);
    if (columns != count) return false;
    for (size_t i = 0; i < count; i++) {
        if (strncmp((const char *)blob.ptr + 16 + 40 * i, want[i], 16) != 0) return false;
    }
    return true;
}

static bool bytes_are(tg_bytes b, const char *want) {
    return b.len == strlen(want) && (b.len == 0 || memcmp(b.ptr, want, b.len) == 0);
}

/* The last error's message is `want`. */
static bool message_is(const char *want) {
    tg_bytes msg = {0};
    tg_last_error(&msg);
    bool same = bytes_are(msg, want);
    tg_bytes_free(msg);
    return same;
}

static double distance(double a, double b) { return a > b ? a - b : b - a; }

static bool upto3(void *ctx, uint32_t step) {
    *(uint32_t *)ctx += step;
    return step < 3;
}

static bool before(void *ctx, uint32_t step) { return step < *(const uint32_t *)ctx; }

/* The ids a query returns are exactly `want`. */
#define IDS_ARE(call, want) do {                                         \
        tg_bytes ids_ = {0};                                             \
        CHECK((call) == TG_OK);                                          \
        static const char *const want_[] = {want};                       \
        CHECK(strs_are(ids_, "id", want_, COUNT(want_)));                \
        tg_bytes_free(ids_);                                             \
    } while (0)

static void library(void) {
    tg_bytes out = {0};
    CHECK(tg_version(&out) == TG_OK && bytes_are(out, TGP_VERSION));
    tg_bytes_free(out);
    CHECK(tg_components(&out) == TG_OK && bytes_are(out, TGP_COMPONENTS));
    tg_bytes_free(out);
    CHECK(tg_noop() == TG_OK);

    static const char *const fold[][2] = {TGP_FOLD_ITEMS};
    CHECK(COUNT(fold) == TGP_FOLD_COUNT);
    for (size_t i = 0; i < COUNT(fold); i++) {
        CHECK(tg_fold(TEXT(fold[i][0]), &out) == TG_OK && bytes_are(out, fold[i][1]));
        tg_bytes_free(out);
    }
    const uint8_t bad[] = {0xff};
    CHECK(tg_fold(bad, 1, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_bytes msg = {0};
    CHECK(tg_last_error(&msg) == TG_ERROR_INVALID_ARGUMENT && msg.len > 0);
    tg_bytes_free(msg);

    CHECK(tg_sample_rows(TGP_SAMPLE_ROWS_ROWS, &out) == TG_OK && out.len == TGP_SAMPLE_ROWS_BYTES);
    CHECK(memcmp(out.ptr, "TGB1", 4) == 0 && ((uintptr_t)out.ptr % 8) == 0);
    CHECK(blob_rows(out) == TGP_SAMPLE_ROWS_ROWS);
    static const char *const sample_columns[] = {TGP_SAMPLE_ROWS_COLUMNS_ITEMS};
    CHECK(columns_are(out, sample_columns, COUNT(sample_columns)));
    double lat = 0.0, elevation = 0.0;
    for (uint32_t r = 0; r < blob_rows(out); r++) { /* in row order, as Rust sums */
        lat += column_num(out, "lat", r);
        elevation += column_num(out, "elevation", r);
    }
    CHECK(lat == TGP_SAMPLE_ROWS_LAT_SUM && elevation == TGP_SAMPLE_ROWS_ELEVATION_SUM);
    CHECK(str_is(out, "name", blob_rows(out) - 1, TGP_SAMPLE_ROWS_LAST_NAME));
    tg_bytes_free(out);
    CHECK(TG_SAMPLE_ROWS_MAX == TGP_SAMPLE_ROWS_MAX);
    CHECK(tg_sample_rows(TG_SAMPLE_ROWS_MAX + 1, &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);

    uint32_t sum = 0, steps = 0;
    CHECK(tg_progress(2, upto3, &sum, &steps) == TG_OK && steps == 2 && sum == 3);
    CHECK(tg_progress(9, upto3, &sum, &steps) == TG_ERROR_CANCELLED && steps == 2);
    CHECK(tg_last_error(NULL) == TG_ERROR_CANCELLED);
    uint32_t cancel_after = TGP_PROGRESS_CANCEL_AFTER;
    CHECK(tg_progress(TGP_PROGRESS_STEPS, before, &cancel_after, &steps) == TG_ERROR_CANCELLED);
    CHECK(message_is(TGP_PROGRESS_CANCELLED_MESSAGE));
    CHECK(tg_progress(1, NULL, NULL, &steps) == TG_ERROR_INVALID_ARGUMENT);
    tg_bytes_free((tg_bytes){0});

    static const double args[] = {TGP_DISTANCE_M_ARGS_ITEMS};
    static const double off[] = {TGP_DISTANCE_M_OUT_OF_RANGE_ITEMS};
    double metres = 0.0;
    CHECK(tg_distance_m(args[0], args[1], args[2], args[3], &metres) == TG_OK);
    CHECK(distance(metres, TGP_DISTANCE_M_METRES) <= 1e-6);
    CHECK(tg_distance_m(off[0], off[1], off[2], off[3], &metres) == TG_ERROR_INVALID_ARGUMENT);

    CHECK(TG_ERROR_INVALID_ARGUMENT == TGP_ERROR_CODES_INVALIDARGUMENT);
    CHECK(TG_ERROR_CANCELLED == TGP_ERROR_CODES_CANCELLED);
    CHECK(TG_ERROR_INTERNAL == TGP_ERROR_CODES_INTERNAL);
    CHECK(TG_ERROR_IO == TGP_ERROR_CODES_IO);
    CHECK(TG_ERROR_CORRUPT == TGP_ERROR_CODES_CORRUPT);
    CHECK(TG_ERROR_MISSING_COMPONENT == TGP_ERROR_CODES_MISSINGCOMPONENT);
    CHECK(TG_ERROR_NETWORK == TGP_ERROR_CODES_NETWORK);
    CHECK(TG_ERROR_REFUSED == TGP_ERROR_CODES_REFUSED);
}

static void engine_queries(void) {
    const char *path = FIXTURE(TGP_ENGINE_FIXTURE), *id = TGP_ENGINE_GET_ID;
    tg_bytes out = {0};
    tg_engine *engine = NULL;
    CHECK(tg_engine_new(&engine) == TG_OK && engine != NULL);
    uint32_t layers = 9;
    CHECK(tg_engine_open_file(engine, TEXT(path), &layers) == TG_OK && layers == 0);

    CHECK(tg_engine_get(engine, TEXT(id), &out) == TG_OK && blob_rows(out) == 1);
    static const char *const place_columns[] = {TGP_ENGINE_COLUMNS_ITEMS};
    CHECK(columns_are(out, place_columns, COUNT(place_columns)));
    CHECK(str_is(out, "name", 0, TGP_ENGINE_GET_NAME) && str_is(out, "kind", 0, TGP_ENGINE_GET_KIND));
    CHECK(column_num(out, "lat", 0) == TGP_ENGINE_GET_LAT && column_num(out, "lon", 0) == TGP_ENGINE_GET_LON);
    tg_bytes_free(out);

    CHECK(tg_engine_search(engine, TEXT(TGP_ENGINE_SEARCH_QUERY), TGP_ENGINE_SEARCH_LIMIT, &out) == TG_OK);
    CHECK(str_is(out, "id", 0, TGP_ENGINE_SEARCH_FIRST_ID));
    tg_bytes_free(out);

    IDS_ARE(tg_engine_reverse(engine, TGP_ENGINE_REVERSE_LAT, TGP_ENGINE_REVERSE_LON, TGP_ENGINE_REVERSE_LIMIT, &ids_),
            TGP_ENGINE_REVERSE_IDS_ITEMS);
    CHECK(tg_engine_reverse(engine, TGP_ENGINE_REVERSE_LAT, TGP_ENGINE_REVERSE_LON, TGP_ENGINE_REVERSE_LIMIT, &out) == TG_OK);
    CHECK(distance(column_num(out, "meters", 0), TGP_ENGINE_REVERSE_FIRST_METERS) <= 0.1);
    tg_bytes_free(out);
    IDS_ARE(tg_engine_admin_at(engine, TGP_ENGINE_ADMIN_AT_LAT, TGP_ENGINE_ADMIN_AT_LON, &ids_), TGP_ENGINE_ADMIN_AT_IDS_ITEMS);
    IDS_ARE(tg_engine_parent(engine, TEXT(TGP_ENGINE_PARENT_ID), &ids_), TGP_ENGINE_PARENT_IDS_ITEMS);
    IDS_ARE(tg_engine_ancestors(engine, TEXT(TGP_ENGINE_ANCESTORS_ID), &ids_), TGP_ENGINE_ANCESTORS_IDS_ITEMS);
    IDS_ARE(tg_engine_children(engine, TEXT(TGP_ENGINE_CHILDREN_ID), &ids_), TGP_ENGINE_CHILDREN_IDS_ITEMS);
    IDS_ARE(tg_engine_nearby(engine, TEXT(TGP_ENGINE_NEARBY_ID), TGP_ENGINE_NEARBY_LIMIT, &ids_), TGP_ENGINE_NEARBY_IDS_ITEMS);
    IDS_ARE(tg_engine_within_radius(engine, TGP_ENGINE_WITHIN_RADIUS_LAT, TGP_ENGINE_WITHIN_RADIUS_LON,
                                    TGP_ENGINE_WITHIN_RADIUS_METERS, TGP_ENGINE_WITHIN_RADIUS_LIMIT, &ids_),
            TGP_ENGINE_WITHIN_RADIUS_IDS_ITEMS);
    CHECK(tg_engine_within_radius(engine, TGP_ENGINE_WITHIN_RADIUS_LAT, TGP_ENGINE_WITHIN_RADIUS_LON, -1.0, 1, &out) ==
          TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_tz_at(engine, TGP_ENGINE_TZ_AT_LAT, TGP_ENGINE_TZ_AT_LON, &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "tz", 0, TGP_ENGINE_TZ_AT_TZ));
    tg_bytes_free(out);
    IDS_ARE(tg_engine_by_iso(engine, TEXT(TGP_ENGINE_BY_ISO_CODE), &ids_), TGP_ENGINE_BY_ISO_ID);
    CHECK(tg_engine_by_iso(engine, TEXT(TGP_ENGINE_BAD_ISO), &out) == TG_ERROR_INVALID_ARGUMENT);

    tg_search_options focus = {.has_focus_lat = true, .focus_lat = TGP_ENGINE_SEARCH_WITH_FOCUS_LAT,
                               .has_focus_lon = true, .focus_lon = TGP_ENGINE_SEARCH_WITH_FOCUS_LON,
                               .has_focus_scale_m = true, .focus_scale_m = TGP_ENGINE_SEARCH_WITH_FOCUS_SCALE_M};
    static const char *const focused[] = {TGP_ENGINE_SEARCH_WITH_IDS_ITEMS};
    const char *lalek = TGP_ENGINE_SEARCH_WITH_QUERY;
    CHECK(tg_engine_search_with(engine, TEXT(lalek), TGP_ENGINE_SEARCH_WITH_LIMIT, &focus, &out) == TG_OK);
    CHECK(strs_are(out, "id", focused, COUNT(focused)));
    tg_bytes_free(out);
    CHECK(tg_engine_search_with(engine, TEXT(lalek), TGP_ENGINE_SEARCH_WITH_LIMIT, NULL, &out) == TG_OK);
    CHECK(!strs_are(out, "id", focused, COUNT(focused)));
    tg_bytes_free(out);
    tg_search_options half = {.has_focus_lat = true, .focus_lat = 1.0};
    CHECK(tg_engine_search_with(engine, TEXT(lalek), 1, &half, &out) == TG_ERROR_INVALID_ARGUMENT);

    static const char *const looked_up[] = {TGP_ENGINE_POSTAL_LOOKUP_POSTAL_ITEMS};
    CHECK(tg_engine_postal_lookup(engine, TEXT(TGP_ENGINE_POSTAL_LOOKUP_CODE), TEXT(TGP_ENGINE_POSTAL_LOOKUP_SCHEME), &out) == TG_OK);
    CHECK(strs_are(out, "postal", looked_up, COUNT(looked_up)));
    tg_bytes_free(out);
    IDS_ARE(tg_engine_postal_lookup(engine, TEXT(TGP_ENGINE_POSTAL_LOOKUP_CODE), TEXT(TGP_ENGINE_POSTAL_LOOKUP_SCHEME), &ids_),
            TGP_ENGINE_POSTAL_LOOKUP_IDS_ITEMS);
    static const char *const prefixed[] = {TGP_ENGINE_POSTAL_PREFIX_POSTAL_ITEMS};
    CHECK(tg_engine_postal_prefix(engine, TEXT(TGP_ENGINE_POSTAL_PREFIX_INPUT), TEXT(TGP_ENGINE_POSTAL_PREFIX_SCHEME),
                                  TGP_ENGINE_POSTAL_PREFIX_LIMIT, &out) == TG_OK);
    CHECK(strs_are(out, "postal", prefixed, COUNT(prefixed)));
    tg_bytes_free(out);
    static const char *const at_codes[] = {TGP_ENGINE_POSTAL_AT_POSTAL_ITEMS};
    static const char *const at_kinds[] = {TGP_ENGINE_POSTAL_AT_CODE_KIND_ITEMS};
    CHECK(tg_engine_postal_at(engine, TGP_ENGINE_POSTAL_AT_LAT, TGP_ENGINE_POSTAL_AT_LON, &out) == TG_OK);
    CHECK(strs_are(out, "postal", at_codes, COUNT(at_codes)) && strs_are(out, "code_kind", at_kinds, COUNT(at_kinds)));
    tg_bytes_free(out);
    static const char *const codes_of[] = {TGP_ENGINE_POSTAL_CODES_OF_POSTAL_ITEMS};
    CHECK(tg_engine_postal_codes_of(engine, TEXT(TGP_ENGINE_POSTAL_CODES_OF_ID), &out) == TG_OK);
    CHECK(strs_are(out, "postal", codes_of, COUNT(codes_of)));
    tg_bytes_free(out);

    CHECK(tg_engine_get(engine, TEXT(TGP_ENGINE_BAD_ID), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_last_error(NULL) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_get(NULL, TEXT(id), &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_engine_open_file(engine, TEXT("/nonexistent.tgd"), &layers) == TG_ERROR_IO);
    CHECK(tg_last_error(NULL) == TG_ERROR_IO);
    CHECK(tg_engine_open_bytes(engine, TEXT("not a pack"), &layers) == TG_ERROR_CORRUPT);
    CHECK(tg_engine_layers(engine, &layers) == TG_OK && layers == 1);

    CHECK(tg_engine_open_file(engine, TEXT(FIXTURE(TGP_ENGINE_ELEVATION_AT_FIXTURE)), &layers) == TG_OK && layers == 1);
    CHECK(tg_engine_elevation_at(engine, TGP_ENGINE_ELEVATION_AT_LAT, TGP_ENGINE_ELEVATION_AT_LON, &out) == TG_OK);
    CHECK(blob_rows(out) == 1 && str_is(out, "source", 0, TGP_ENGINE_ELEVATION_AT_SOURCE));
    CHECK(column_num(out, "meters", 0) == TGP_ENGINE_ELEVATION_AT_METERS);
    tg_bytes_free(out);
    bool removed = false;
    CHECK(tg_engine_remove(engine, 1, &removed) == TG_OK && removed);
    CHECK(tg_engine_remove(engine, 1, &removed) == TG_OK && !removed);
    tg_engine_free(engine);
    tg_engine_free(NULL);
}

static void open_options(void) {
    const char *path = FIXTURE(TGP_ENGINE_FIXTURE);
    tg_bytes out = {0};
    uint32_t layers = 0;
    FILE *f = fopen(path, "rb");
    CHECK(f != NULL);
    if (f == NULL) return;
    static uint8_t pack[1 << 20];
    size_t pack_len = fread(pack, 1, sizeof pack, f);
    fclose(f);
    CHECK(pack_len > TGP_ENGINE_OPEN_WITH_CORRUPT_AT && pack_len < sizeof pack);
    pack[TGP_ENGINE_OPEN_WITH_CORRUPT_AT] ^= 0xff;
    tg_engine *scratch = NULL;
    CHECK(tg_engine_new(&scratch) == TG_OK);
    tg_open_options eager = {.verify = TG_VERIFY_EAGER};
    CHECK(tg_engine_open_bytes_with(scratch, pack, pack_len, &eager, &layers) == TG_ERROR_CORRUPT);
    CHECK(tg_engine_open_bytes_with(scratch, pack, pack_len, NULL, &layers) == TG_OK && layers == 0);
    tg_open_options every = {.priority = 2, .verify = TG_VERIFY_EAGER, .load = TG_LOAD_READ, .has_outline_level = true, .outline_level = 1};
    CHECK(tg_engine_open_file_with(scratch, TEXT(path), &every, &layers) == TG_OK && layers == 1);
    tg_open_options odd = {.verify = TGP_ENGINE_OPEN_WITH_BAD_VERIFY};
    CHECK(tg_engine_open_file_with(scratch, TEXT(path), &odd, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_open_options deep = {.has_outline_level = true, .outline_level = TGP_ENGINE_OPEN_WITH_BAD_OUTLINE_LEVEL};
    CHECK(tg_engine_open_file_with(scratch, TEXT(path), &deep, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(scratch);

    tg_engine *points = NULL;
    CHECK(tg_engine_new(&points) == TG_OK);
    const char *point = TGP_ENGINE_OPEN_WITH_COMPONENTS, *langs = TGP_ENGINE_OPEN_WITH_LANGUAGES;
    const char *pointy = TGP_ENGINE_OPEN_WITH_BAD_COMPONENTS;
    tg_open_options only = {.components = (const uint8_t *)point, .components_len = strlen(point),
                            .languages = (const uint8_t *)langs, .languages_len = strlen(langs)};
    CHECK(tg_engine_open_file_with(points, TEXT(path), &only, &layers) == TG_OK);
    CHECK(tg_engine_get(points, TEXT(TGP_ENGINE_GET_ID), &out) == TG_OK && blob_rows(out) == 1);
    tg_bytes_free(out);
    CHECK(tg_engine_search(points, TEXT(TGP_ENGINE_SEARCH_QUERY), 1, &out) == TG_ERROR_MISSING_COMPONENT);
    tg_open_options unknown = {.components = (const uint8_t *)pointy, .components_len = strlen(pointy)};
    CHECK(tg_engine_open_file_with(points, TEXT(path), &unknown, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_open_options broken = {.components = NULL, .components_len = 3};
    CHECK(tg_engine_open_file_with(points, TEXT(path), &broken, &layers) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(points);
}

static void zone_rules(void) {
    tg_bytes out = {0};
    tg_engine *rules = NULL;
    CHECK(tg_engine_new(&rules) == TG_OK);
    uint32_t layer = 9;
    CHECK(tg_engine_open_file(rules, TEXT(FIXTURE(TGP_TZ_FIXTURE)), &layer) == TG_OK && layer == 0);
    CHECK(tg_engine_tz_version(rules, &out) == TG_OK && bytes_are(out, TGP_TZ_VERSION));
    tg_bytes_free(out);
    static const char *const zones[] = {TGP_TZ_ZONES_ITEMS};
    CHECK(tg_engine_tz_zones(rules, &out) == TG_OK && strs_are(out, "zone", zones, COUNT(zones)));
    tg_bytes_free(out);
    CHECK(tg_engine_tz_offset_at(rules, TEXT(TGP_TZ_OFFSET_ZONE), TGP_TZ_OFFSET_UNIX, &out) == TG_OK && blob_rows(out) == 1);
    CHECK(column_num(out, "seconds", 0) == TGP_TZ_OFFSET_SECONDS && column_num(out, "dst", 0) == TGP_TZ_OFFSET_DST);
    CHECK(str_is(out, "abbr", 0, TGP_TZ_OFFSET_ABBR));
    tg_bytes_free(out);
    static const int64_t gap[] = {TGP_TZ_GAP_CIVIL_ITEMS}, overlap[] = {TGP_TZ_OVERLAP_CIVIL_ITEMS};
    CHECK(tg_engine_tz_local(rules, TEXT(TGP_TZ_GAP_ZONE), (int32_t)gap[0], (uint32_t)gap[1], (uint32_t)gap[2],
                             (uint32_t)gap[3], (uint32_t)gap[4], (uint32_t)gap[5], &out) == TG_OK);
    CHECK(str_is(out, "kind", 0, "gap") && column_num(out, "before", 0) == TGP_TZ_GAP_BEFORE &&
          column_num(out, "after", 0) == TGP_TZ_GAP_AFTER);
    tg_bytes_free(out);
    CHECK(tg_engine_tz_local(rules, TEXT(TGP_TZ_OVERLAP_ZONE), (int32_t)overlap[0], (uint32_t)overlap[1], (uint32_t)overlap[2],
                             (uint32_t)overlap[3], (uint32_t)overlap[4], (uint32_t)overlap[5], &out) == TG_OK);
    CHECK(str_is(out, "kind", 0, "overlap") && column_num(out, "before", 0) == TGP_TZ_OVERLAP_BEFORE &&
          column_num(out, "after", 0) == TGP_TZ_OVERLAP_AFTER);
    tg_bytes_free(out);
    CHECK(tg_engine_tz_rules_start(rules, TEXT(TGP_TZ_START_ZONE), &out) == TG_OK);
    CHECK(column_num(out, "unix", 0) == (double)TGP_TZ_START_UNIX);
    tg_bytes_free(out);
    CHECK(tg_engine_tz_offset_at(rules, TEXT(TGP_TZ_UNKNOWN), 0, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_engine_tz_local(rules, TEXT(TGP_TZ_OFFSET_ZONE), 2024, 13, 1, 0, 0, 0, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(rules);
}

#if TEIGEODATA_FEATURE_STORE
/* The store: the fixture root, a mirror that refuses connections. A lite library's header
 * leaves it out. */
static void store_jobs(void) {
    tg_bytes out = {0};
    FILE *rf = fopen(FIXTURE(TGP_STORE_ROOT), "rb");
    CHECK(rf != NULL);
    if (rf == NULL) return;
    static uint8_t root[1 << 16];
    size_t root_len = fread(root, 1, sizeof root, rf);
    fclose(rf);
    const char *mirror = TGP_STORE_MIRROR, *np = TGP_STORE_PACK, *pointy = TGP_STORE_BAD_COMPONENTS;
    const char *missing = "target/c-store-missing", *bad_dir = "target/c-store-bad", *dir = "target/c-store";
    tg_store *store = NULL;
    CHECK(tg_store_open(TEXT(missing), &store) == TG_ERROR_REFUSED);
    CHECK(tg_store_create(TEXT(bad_dir), TEXT(mirror), TEXT(mirror), TEXT("{}"), &store) == TG_ERROR_REFUSED);
    CHECK(tg_store_create(TEXT(dir), TEXT(mirror), TEXT(mirror), root, root_len, &store) == TG_OK);
    tg_store_free(store);
    CHECK(tg_store_open(TEXT(dir), &store) == TG_OK);
    CHECK(tg_store_damaged(store, &out) == TG_OK && out.len == 0);
    tg_bytes_free(out);
    CHECK(tg_store_catalog(store, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_status(store, false, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    tg_engine *installed = NULL;
    uint32_t layers = 9;
    CHECK(tg_store_engine(store, &installed) == TG_OK);
    CHECK(tg_engine_layers(installed, &layers) == TG_OK && layers == 0);
    CHECK(tg_store_find(store, installed, TEXT(np), 3, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_find(store, installed, TEXT(np), 10001, &out) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_locate(store, installed, 27.7, 85.3, 1000, &out) == TG_OK && blob_rows(out) == 0);
    tg_bytes_free(out);
    CHECK(tg_store_locate(store, installed, 91.0, 0.0, 1000, &out) == TG_ERROR_INVALID_ARGUMENT);
    tg_engine_free(installed);
    tg_job *job = NULL;
    tg_install_options wrong = {.components = (const uint8_t *)pointy, .components_len = strlen(pointy)};
    CHECK(tg_store_install(store, TEXT(np), &wrong, &job) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_remove(store, TEXT(np), &out) == TG_ERROR_REFUSED);
    /* A platform's pass (D-184) needs a downloads directory; with one, the catalog's refresh
     * fails on the refusing mirror. */
    CHECK(tg_store_install_pass(store, TEXT(np), TEXT(""), NULL, &job) == TG_ERROR_INVALID_ARGUMENT);
    CHECK(tg_store_install_pass(store, TEXT(np), TEXT("target/c-store-dl"), NULL, &job) == TG_OK);
    CHECK(tg_job_result(job, &out) == TG_ERROR_NETWORK);
    tg_job_free(job);
    CHECK(tg_store_refresh(store, TEXT(np), &job) == TG_OK);
    CHECK(tg_job_result(job, &out) == TG_ERROR_NETWORK);
    bool ended = false;
    CHECK(tg_job_is_ended(job, &ended) == TG_OK && ended);
    uint64_t seq = 0, next = 0, done = 0, total = 1;
    CHECK(tg_job_seq(job, &seq) == TG_OK && tg_job_wait_progress(job, seq, 60000, &next) == TG_OK && next == seq);
    CHECK(tg_job_done_bytes(job, &done) == TG_OK && tg_job_total_bytes(job, &total) == TG_OK && done == total);
    CHECK(tg_job_cancel(job) == TG_OK);
    tg_job_free(job);
    tg_store_free(store);
}
#endif

int main(void) {
    library();
    engine_queries();
    open_options();
    zone_rules();
#if TEIGEODATA_FEATURE_STORE
    store_jobs();
#endif
    if (failures == 0) puts("c parity ok");
    return failures != 0;
}
