/*
 * Reads a teigeodata result blob (format v1, or v0; D-275) in place: header-only C99, no
 * allocation, nothing copied. Hand-written: the blob format is not part of the IDL. The same
 * shape as every language's reader (D-322).
 *
 *     tg_bytes blob = {0};
 *     tg_engine_search(engine, (const uint8_t *)"Pokhara", 7, 5, &blob);
 *     tg_reader r;
 *     if (tg_reader_init(&r, blob.ptr, blob.len) == TG_OK) {
 *         int32_t name = tg_reader_find(&r, "name"), lat = tg_reader_find(&r, "lat");
 *         for (uint32_t row = 0; row < r.rows; row++) {
 *             tg_str s = tg_reader_string(&r, name, row);
 *             printf("%.*s %f\n", (int)s.len, s.ptr, tg_reader_number(&r, lat, row));
 *         }
 *     }
 *     tg_bytes_free(blob);
 *
 * tg_reader_init checks the whole layout once, every string offset and dictionary code
 * included, so no read through a reader it accepted leaves the blob. The reader borrows the
 * blob: free the blob only when done with the reader. A column index is from tg_reader_find
 * (-1 when there is none: the accessors answer null, 0 or an empty string for it, as for a row
 * past the end).
 */
#ifndef TEIGEODATA_READER_H
#define TEIGEODATA_READER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The column types of format v1. */
#define TG_COLUMN_I32 1u
#define TG_COLUMN_U32 2u
#define TG_COLUMN_I64 3u
#define TG_COLUMN_U64 4u
#define TG_COLUMN_F32 5u
#define TG_COLUMN_F64 6u
#define TG_COLUMN_UTF8 7u
#define TG_COLUMN_U8 8u
#define TG_COLUMN_I16 9u
#define TG_COLUMN_U16 10u
#define TG_COLUMN_DICT16 11u
#define TG_COLUMN_DICT32 12u

/* At most this many columns in a blob the reader accepts (results have at most 13). */
#define TG_READER_COLUMNS_MAX 64u

/* A string in the blob: UTF-8, not NUL-terminated. ptr is NULL for a null value. */
typedef struct tg_str {
    const char *ptr;
    size_t len;
} tg_str;

/* A blob's checked layout. Fill it with tg_reader_init; read its fields, never write them. */
typedef struct tg_reader {
    const uint8_t *data;
    size_t len;
    /* The number of rows. */
    uint32_t rows;
    /* The number of columns. */
    uint32_t columns;
} tg_reader;

/* A little-endian u32 at data + at. */
static inline uint32_t tg_reader_u32_(const uint8_t *data, size_t at) {
    return (uint32_t)data[at] | (uint32_t)data[at + 1] << 8 | (uint32_t)data[at + 2] << 16 |
           (uint32_t)data[at + 3] << 24;
}

static inline uint64_t tg_reader_u64_(const uint8_t *data, size_t at) {
    return (uint64_t)tg_reader_u32_(data, at) | (uint64_t)tg_reader_u32_(data, at + 4) << 32;
}

/* Bytes per row of a column type's data: an offset for UTF-8, a code for a dictionary; 0 for
 * no such type. */
static inline size_t tg_reader_width_(uint32_t type) {
    switch (type) {
    case TG_COLUMN_U8: return 1;
    case TG_COLUMN_I16: case TG_COLUMN_U16: case TG_COLUMN_DICT16: return 2;
    case TG_COLUMN_I32: case TG_COLUMN_U32: case TG_COLUMN_F32: case TG_COLUMN_UTF8: case TG_COLUMN_DICT32: return 4;
    case TG_COLUMN_I64: case TG_COLUMN_U64: case TG_COLUMN_F64: return 8;
    default: return 0;
    }
}

/* The descriptor field `field` (0 type, 1 at, 2 len, 3 aux, 4 aux_len, 5 nulls_at) of column
 * `column`. */
static inline uint32_t tg_reader_field_(const tg_reader *r, uint32_t column, size_t field) {
    return tg_reader_u32_(r->data, 16 + 40 * (size_t)column + 16 + 4 * field);
}

/* Whether [at, at + n) lies after the descriptors, inside the blob, 8-aligned. */
static inline bool tg_reader_inside_(size_t len, size_t floor, size_t at, size_t n) {
    return at >= floor && at % 8 == 0 && at <= len && n <= len - at;
}

/* Whether a string column's offsets and codes all point inside its strings. */
static inline bool tg_reader_strings_ok_(const uint8_t *d, uint32_t type, size_t at, size_t aux,
                                         size_t aux_len, uint32_t rows) {
    if (type == TG_COLUMN_UTF8) {
        uint32_t prev = tg_reader_u32_(d, at);
        for (uint32_t i = 1; i <= rows; i++) {
            uint32_t next = tg_reader_u32_(d, at + 4 * (size_t)i);
            if (next < prev || next > aux_len) return false;
            prev = next;
        }
        return prev <= aux_len;
    }
    if (aux_len < 8) return false;
    uint32_t count = tg_reader_u32_(d, aux);
    if (count > (aux_len - 8) / 4) return false;
    size_t base = 8 + 4 * (size_t)count;
    for (uint32_t k = 0; k < count; k++) {
        uint32_t from = tg_reader_u32_(d, aux + 4 + 4 * (size_t)k), to = tg_reader_u32_(d, aux + 8 + 4 * (size_t)k);
        if (from > to || to > aux_len - base) return false;
    }
    for (uint32_t i = 0; i < rows; i++) {
        uint32_t code = type == TG_COLUMN_DICT16
                            ? (uint32_t)d[at + 2 * (size_t)i] | (uint32_t)d[at + 2 * (size_t)i + 1] << 8
                            : tg_reader_u32_(d, at + 4 * (size_t)i);
        if (code >= count && !(code == 0 && count == 0)) return false;
    }
    return true;
}

/* Checks the blob's layout and fills `r`. Returns TG_OK (0), or TG_ERROR_CORRUPT (5) for bytes
 * that are not a well-formed blob, `r` then zeroed. */
static inline int32_t tg_reader_init(tg_reader *r, const uint8_t *data, size_t len) {
    memset(r, 0, sizeof *r);
    if (data == NULL || len < 16) return 5;
    bool v0 = memcmp(data, "TGB0", 4) == 0;
    if (!v0 && memcmp(data, "TGB1", 4) != 0) return 5;
    uint32_t rows = tg_reader_u32_(data, 4), count = tg_reader_u32_(data, 8);
    /* At least one column: with none, nothing would bound the row count. */
    if (tg_reader_u32_(data, 12) != len || count == 0 || count > TG_READER_COLUMNS_MAX || count > (len - 16) / 40) return 5;
    size_t floor = 16 + 40 * (size_t)count;
    tg_reader probe = {data, len, rows, count};
    for (uint32_t i = 0; i < count; i++) {
        const uint8_t *name = data + 16 + 40 * (size_t)i;
        size_t end = 0;
        while (end < 16 && name[end] != 0) end++;
        if (end == 0) return 5;
        for (size_t k = end; k < 16; k++) if (name[k] != 0) return 5;
        for (uint32_t j = 0; j < i; j++) if (memcmp(name, data + 16 + 40 * (size_t)j, 16) == 0) return 5;
        uint32_t type = tg_reader_field_(&probe, i, 0);
        size_t at = tg_reader_field_(&probe, i, 1), n = tg_reader_field_(&probe, i, 2);
        size_t aux = tg_reader_field_(&probe, i, 3), aux_len = tg_reader_field_(&probe, i, 4);
        size_t nulls = tg_reader_field_(&probe, i, 5);
        size_t width = tg_reader_width_(type);
        bool text = type == TG_COLUMN_UTF8 || type == TG_COLUMN_DICT16 || type == TG_COLUMN_DICT32;
        size_t values = (size_t)rows + (type == TG_COLUMN_UTF8 ? 1 : 0);
        if (width == 0 || (v0 && (nulls != 0 || type > TG_COLUMN_UTF8))) return 5;
        if (n / width != values || n % width != 0 || !tg_reader_inside_(len, floor, at, n)) return 5;
        if (text ? !tg_reader_inside_(len, floor, aux, aux_len) : (aux != 0 || aux_len != 0)) return 5;
        if (nulls != 0 && !tg_reader_inside_(len, floor, nulls, ((size_t)rows + 7) / 8)) return 5;
        if (text && !tg_reader_strings_ok_(data, type, at, aux, aux_len, rows)) return 5;
    }
    *r = probe;
    return 0;
}

/* The index of the column called `name` (NUL-terminated), or -1. */
static inline int32_t tg_reader_find(const tg_reader *r, const char *name) {
    size_t n = strlen(name);
    if (n == 0 || n > 16) return -1;
    for (uint32_t i = 0; i < r->columns; i++) {
        const uint8_t *at = r->data + 16 + 40 * (size_t)i;
        if (memcmp(at, name, n) == 0 && (n == 16 || at[n] == 0)) return (int32_t)i;
    }
    return -1;
}

/* The name of column `column` (at most 16 bytes, not NUL-terminated). */
static inline tg_str tg_reader_name(const tg_reader *r, int32_t column) {
    tg_str s = {NULL, 0};
    if (column < 0 || (uint32_t)column >= r->columns) return s;
    s.ptr = (const char *)(r->data + 16 + 40 * (size_t)column);
    while (s.len < 16 && s.ptr[s.len] != 0) s.len++;
    return s;
}

/* The type of column `column` (TG_COLUMN_*), 0 for none. */
static inline uint32_t tg_reader_type(const tg_reader *r, int32_t column) {
    if (column < 0 || (uint32_t)column >= r->columns) return 0;
    return tg_reader_field_(r, (uint32_t)column, 0);
}

/* Whether column `column` holds strings. */
static inline bool tg_reader_is_text(const tg_reader *r, int32_t column) {
    uint32_t t = tg_reader_type(r, column);
    return t == TG_COLUMN_UTF8 || t == TG_COLUMN_DICT16 || t == TG_COLUMN_DICT32;
}

/* Whether column `column` is null at `row` (true for no such column or row). */
static inline bool tg_reader_is_null(const tg_reader *r, int32_t column, uint32_t row) {
    if (column < 0 || (uint32_t)column >= r->columns || row >= r->rows) return true;
    size_t nulls = tg_reader_field_(r, (uint32_t)column, 5);
    return nulls != 0 && ((r->data[nulls + row / 8] >> (row % 8)) & 1) == 0;
}

/* A number column's value at `row` as a double (exact to 2^53 for a 64-bit integer); 0 where
 * null, for a string column, and for no such column or row. */
static inline double tg_reader_number(const tg_reader *r, int32_t column, uint32_t row) {
    if (column < 0 || (uint32_t)column >= r->columns || row >= r->rows) return 0;
    uint32_t type = tg_reader_field_(r, (uint32_t)column, 0);
    size_t at = (size_t)tg_reader_field_(r, (uint32_t)column, 1) + tg_reader_width_(type) * row;
    const uint8_t *d = r->data;
    switch (type) {
    case TG_COLUMN_I32: return (double)(int32_t)tg_reader_u32_(d, at);
    case TG_COLUMN_U32: return (double)tg_reader_u32_(d, at);
    case TG_COLUMN_I64: return (double)(int64_t)tg_reader_u64_(d, at);
    case TG_COLUMN_U64: return (double)tg_reader_u64_(d, at);
    case TG_COLUMN_F32: {
        uint32_t bits = tg_reader_u32_(d, at);
        float f;
        memcpy(&f, &bits, sizeof f);
        return (double)f;
    }
    case TG_COLUMN_F64: {
        uint64_t bits = tg_reader_u64_(d, at);
        double f;
        memcpy(&f, &bits, sizeof f);
        return f;
    }
    case TG_COLUMN_U8: return (double)d[at];
    case TG_COLUMN_I16: return (double)(int16_t)(uint16_t)(d[at] | d[at + 1] << 8);
    case TG_COLUMN_U16: return (double)(uint16_t)(d[at] | d[at + 1] << 8);
    default: return 0;
    }
}

/* An integer column's value at `row` (a u64 above INT64_MAX wraps; a float truncates); 0 as
 * for tg_reader_number. */
static inline int64_t tg_reader_int(const tg_reader *r, int32_t column, uint32_t row) {
    uint32_t type = tg_reader_type(r, column);
    if (row >= r->rows) return 0;
    if (type == TG_COLUMN_I64 || type == TG_COLUMN_U64) {
        size_t at = (size_t)tg_reader_field_(r, (uint32_t)column, 1) + 8 * (size_t)row;
        return (int64_t)tg_reader_u64_(r->data, at);
    }
    return (int64_t)tg_reader_number(r, column, row);
}

/* A string column's value at `row`, in the blob; {NULL, 0} where null, for a number column and
 * for no such column or row. */
static inline tg_str tg_reader_string(const tg_reader *r, int32_t column, uint32_t row) {
    tg_str s = {NULL, 0};
    if (!tg_reader_is_text(r, column) || tg_reader_is_null(r, column, row)) return s;
    uint32_t type = tg_reader_field_(r, (uint32_t)column, 0);
    size_t at = tg_reader_field_(r, (uint32_t)column, 1), aux = tg_reader_field_(r, (uint32_t)column, 3);
    const uint8_t *d = r->data;
    size_t from, to;
    if (type == TG_COLUMN_UTF8) {
        from = aux + tg_reader_u32_(d, at + 4 * (size_t)row);
        to = aux + tg_reader_u32_(d, at + 4 * (size_t)row + 4);
    } else {
        uint32_t count = tg_reader_u32_(d, aux);
        uint32_t code = type == TG_COLUMN_DICT16 ? (uint32_t)(d[at + 2 * (size_t)row] | d[at + 2 * (size_t)row + 1] << 8)
                                                 : tg_reader_u32_(d, at + 4 * (size_t)row);
        if (code >= count) return s;
        size_t base = aux + 8 + 4 * (size_t)count;
        from = base + tg_reader_u32_(d, aux + 4 + 4 * (size_t)code);
        to = base + tg_reader_u32_(d, aux + 8 + 4 * (size_t)code);
    }
    s.ptr = (const char *)(d + from);
    s.len = to - from;
    return s;
}

#ifdef __cplusplus
}
#endif

#endif /* TEIGEODATA_READER_H */
