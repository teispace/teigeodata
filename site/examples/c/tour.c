/* The tour: every query of the guide, run on a released pack.
 *
 * Usage: cc -std=c11 tour.c -I<include> -L<lib> -lteigeodata -o tour && ./tour world-np-lite.tgd
 */

/* #region import */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "teigeodata.h"
#include "teigeodata_reader.h"
/* #endregion import */

/* #region helpers */
#define TEXT(s) (const uint8_t *)(s), strlen(s)

/* Exits with the thread's last error when a call failed. */
static void check(int32_t status) {
    if (status == TG_OK) return;
    tg_bytes message = {0};
    tg_last_error(&message);
    fprintf(stderr, "error %d: %.*s\n", (int)status, (int)message.len, (const char *)message.ptr);
    tg_bytes_free(message);
    exit(1);
}

/* A reader over a result the call just wrote into `blob`. */
static tg_reader reader(int32_t status, const tg_bytes *blob) {
    check(status);
    tg_reader r;
    check(tg_reader_init(&r, blob->ptr, blob->len));
    return r;
}

/* Column `name` of row `row`: a string ("" where null), a number, an integer. */
static tg_str str(const tg_reader *r, const char *name, uint32_t row) {
    tg_str s = tg_reader_string(r, tg_reader_find(r, name), row);
    if (s.ptr == NULL) s.ptr = "";
    return s;
}
static double num(const tg_reader *r, const char *name, uint32_t row) {
    return tg_reader_number(r, tg_reader_find(r, name), row);
}
static long long integer(const tg_reader *r, const char *name, uint32_t row) {
    return (long long)tg_reader_int(r, tg_reader_find(r, name), row);
}
#define S(r, name, row) (int)str(r, name, row).len, str(r, name, row).ptr
/* #endregion helpers */

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    const char *pack = argv[1];
    tg_bytes blob = {0};
    tg_reader r;

    puts("=== open");
    /* #region open */
    tg_engine *engine = NULL;
    check(tg_engine_new(&engine));
    uint32_t layer = 0, layers = 0;
    check(tg_engine_open_file(engine, TEXT(pack), &layer));
    check(tg_engine_layers(engine, &layers));
    printf("layers: %u\n", layers);
    /* #endregion open */

    puts("=== search");
    /* #region search */
    r = reader(tg_engine_search(engine, TEXT("Pokhara"), 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.*s  %.4f, %.4f  score %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i),
               num(&r, "lat", i), num(&r, "lon", i), integer(&r, "score", i));
    }
    tg_bytes_free(blob);
    /* #endregion search */

    puts("=== search-parent");
    /* #region search-parent */
    r = reader(tg_engine_search(engine, TEXT("Kathmandu, Bagmati"), 2, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.*s %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i), integer(&r, "level", i));
    }
    tg_bytes_free(blob);
    /* #endregion search-parent */

    puts("=== search-focus");
    /* #region search-focus */
    /* Many places share a name; a focus (the user's location) ranks the near ones first. */
    tg_search_options options = {0};
    options.has_focus_lat = true;
    options.focus_lat = 28.7;
    options.has_focus_lon = true;
    options.focus_lon = 80.6;
    options.has_focus_scale_m = true;
    options.focus_scale_m = 30000;
    r = reader(tg_engine_search_with(engine, TEXT("Shivapur"), 3, &options, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.4f, %.4f\n", S(&r, "id", i), S(&r, "name", i), num(&r, "lat", i), num(&r, "lon", i));
    }
    tg_bytes_free(blob);
    /* #endregion search-focus */

    puts("=== get");
    /* #region get */
    r = reader(tg_engine_get(engine, TEXT("tgd:np/1xhb"), &blob), &blob);
    printf("%.*s %.*s\n", S(&r, "name", 0), S(&r, "kind", 0));
    printf("at %.6f, %.6f, %lld m\n", num(&r, "lat", 0), num(&r, "lon", 0), integer(&r, "elevation", 0));
    printf("timezone %.*s parent %.*s\n", S(&r, "tz", 0), S(&r, "parent", 0));
    tg_bytes_free(blob);
    /* #endregion get */

    puts("=== hierarchy");
    /* #region hierarchy */
    r = reader(tg_engine_ancestors(engine, TEXT("tgd:np/1xhb"), &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s %lld  %.*s", S(&r, "kind", i), integer(&r, "level", i), S(&r, "name", i));
        if (!tg_reader_is_null(&r, tg_reader_find(&r, "iso"), i)) printf("  %.*s", S(&r, "iso", i));
        putchar('\n');
    }
    tg_bytes_free(blob);
    r = reader(tg_engine_children(engine, TEXT("tgd:np/295c"), &blob), &blob);
    printf("Kaski has %u children\n", r.rows);
    tg_bytes_free(blob);
    /* #endregion hierarchy */

    puts("=== reverse");
    /* #region reverse */
    r = reader(tg_engine_reverse(engine, 27.7172, 85.3240, 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.0f m at %.0f°\n", S(&r, "name", i), num(&r, "meters", i), num(&r, "bearing", i));
    }
    tg_bytes_free(blob);
    /* #endregion reverse */

    puts("=== radius");
    /* #region radius */
    r = reader(tg_engine_within_radius(engine, 27.7172, 85.3240, 500, 10, &blob), &blob);
    printf("%u places within 500 m; farthest %.0f m\n", r.rows, num(&r, "meters", r.rows - 1));
    tg_bytes_free(blob);
    /* #endregion radius */

    puts("=== nearby");
    /* #region nearby */
    r = reader(tg_engine_nearby(engine, TEXT("tgd:np/2cn"), 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) printf("%.*s  %.0f m\n", S(&r, "name", i), num(&r, "meters", i));
    tg_bytes_free(blob);
    /* #endregion nearby */

    puts("=== admin");
    /* #region admin */
    r = reader(tg_engine_admin_at(engine, 28.2096, 83.9856, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%lld  %.*s", integer(&r, "level", i), S(&r, "name", i));
        if (!tg_reader_is_null(&r, tg_reader_find(&r, "iso"), i)) printf("  %.*s", S(&r, "iso", i));
        putchar('\n');
    }
    tg_bytes_free(blob);
    r = reader(tg_engine_by_iso(engine, TEXT("NP-P4"), &blob), &blob);
    printf("NP-P4 is %.*s %.*s\n", S(&r, "name", 0), S(&r, "id", 0));
    tg_bytes_free(blob);
    /* #endregion admin */

    puts("=== timezone");
    /* #region timezone */
    tg_bytes area = {0}, at = {0}, local = {0}, version = {0};
    tg_reader zr = reader(tg_engine_tz_at(engine, 27.7172, 85.3240, &area), &area);
    tg_str zone = str(&zr, "tz", 0);
    r = reader(tg_engine_tz_offset_at(engine, (const uint8_t *)zone.ptr, zone.len, 1700000000, &at), &at);
    printf("%.*s %lld s east of UTC, %.*s dst %lld\n", (int)zone.len, zone.ptr, integer(&r, "seconds", 0),
           S(&r, "abbr", 0), integer(&r, "dst", 0));
    r = reader(tg_engine_tz_local(engine, TEXT("America/New_York"), 2026, 3, 8, 2, 30, 0, &local), &local);
    printf("02:30 on 2026-03-08 in New York: %.*s %lld -> %lld\n", S(&r, "kind", 0), integer(&r, "before", 0),
           integer(&r, "after", 0));
    check(tg_engine_tz_version(engine, &version));
    printf("tzdb %.*s\n", (int)version.len, (const char *)version.ptr);
    tg_bytes_free(area);
    tg_bytes_free(at);
    tg_bytes_free(local);
    tg_bytes_free(version);
    /* #endregion timezone */

    puts("=== elevation");
    /* #region elevation */
    r = reader(tg_engine_elevation_at(engine, 27.7172, 85.3240, &blob), &blob);
    printf("%.1f m above %.*s (%.*s, %.0f m grid)\n", num(&r, "meters", 0), S(&r, "datum", 0), S(&r, "source", 0),
           num(&r, "resolution_m", 0));
    tg_bytes_free(blob);
    /* #endregion elevation */

    puts("=== distance");
    /* #region distance */
    double meters = 0;
    check(tg_distance_m(27.7172, 85.3240, 28.2096, 83.9856, &meters));
    double heading = 0;
    check(tg_bearing_deg(27.7172, 85.3240, 28.2096, 83.9856, &heading));
    printf("Kathmandu to Pokhara: %.1f km, heading %.0f°\n", meters / 1000, heading);
    r = reader(tg_engine_distance(engine, TEXT("tgd:np/2cn"), TEXT("tgd:np/1xhb"), &blob), &blob);
    printf("place to place: %.1f km at %.0f°\n", num(&r, "meters", 0) / 1000, num(&r, "bearing", 0));
    tg_bytes_free(blob);
    /* #endregion distance */

    puts("=== boundary");
    /* #region boundary */
    r = reader(tg_engine_boundary(engine, TEXT("tgd:np/2gm"), 0, &blob), &blob);
    tg_str geojson = str(&r, "geojson", 0); /* a GeoJSON Feature, for any map library */
    const char *type = strstr(geojson.ptr, "\"geometry\":{\"type\":\"") + 20;
    printf("%.*s of %lld vertices at level %lld\n", (int)strcspn(type, "\""), type, integer(&r, "vertices", 0),
           integer(&r, "lod", 0));
    printf("tolerance %.0f m, %zu bytes of GeoJSON\n", num(&r, "tolerance_m", 0), geojson.len);
    tg_bytes_free(blob);
    /* #endregion boundary */

    puts("=== errors");
    /* #region errors */
    if (tg_engine_reverse(engine, 91, 0, 1, &blob) == TG_ERROR_INVALID_ARGUMENT) {
        tg_bytes message = {0};
        tg_last_error(&message);
        printf("invalid argument: %.*s\n", (int)message.len, (const char *)message.ptr);
        tg_bytes_free(message);
    }
    /* #endregion errors */

    tg_engine_free(engine);
    return 0;
}
