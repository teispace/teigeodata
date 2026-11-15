/* The parity suite in C++ through the generated wrapper (teigeodata.hpp, 5.8d): every value of
 * bindings/parity.json, read from the generated parity_data.h, and what the wrapper adds:
 * exceptions with their codes, a callback's exception carried across the library, records
 * with their defaults, move-only handles. Run it from the repository root:
 *
 *   c++ -std=c++17 -Wall -Wextra -Werror -pedantic bindings/c/parity.cpp -Ibindings/c \
 *       -Ltarget/release -lteigeodata_ffi -o target/cpp-parity && target/cpp-parity
 */
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "parity_data.h"
#include "teigeodata.hpp"
#include "teigeodata_reader.hpp"

namespace tg = teigeodata;

static int failures = 0;
#define CHECK(cond)                                                                  \
    do {                                                                             \
        if (!(cond)) {                                                               \
            std::fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                              \
        }                                                                            \
    } while (0)
/* A fixture path: parity.json's paths are relative to bindings/. */
#define FIXTURE(rel) "bindings/" rel

using Strings = std::vector<std::string>;

/// The code a call throws, or none.
template <typename F>
static std::optional<tg::ErrorCode> code_of(F &&call) {
    try {
        call();
    } catch (const tg::Error &e) {
        return e.code();
    }
    return std::nullopt;
}

/// The message a call throws, or none.
template <typename F>
static std::optional<std::string> message_of(F &&call) {
    try {
        call();
    } catch (const tg::Error &e) {
        return std::string(e.what());
    }
    return std::nullopt;
}

static std::uint32_t u32_at(const tg::Blob &b, std::size_t at) {
    std::uint32_t v;
    std::memcpy(&v, b.data() + at, 4);
    return v;
}

static std::uint32_t rows(const tg::Blob &b) { return u32_at(b, 4); }

/// The descriptor of column `name`: type, data, data length, aux, aux length, nulls.
static std::optional<std::vector<std::uint32_t>> column(const tg::Blob &b, const char *name) {
    const std::uint32_t columns = u32_at(b, 8);
    for (std::uint32_t i = 0; i < columns; i++) {
        const std::size_t d = 16 + 40 * std::size_t{i};
        if (std::strncmp(reinterpret_cast<const char *>(b.data() + d), name, 16) != 0) continue;
        std::vector<std::uint32_t> f(6);
        std::memcpy(f.data(), b.data() + d + 16, 6 * sizeof(std::uint32_t));
        return f;
    }
    return std::nullopt;
}

static Strings columns(const tg::Blob &b) {
    Strings names;
    for (std::uint32_t i = 0; i < u32_at(b, 8); i++) {
        const char *at = reinterpret_cast<const char *>(b.data() + 16 + 40 * std::size_t{i});
        names.emplace_back(at, strnlen(at, 16));
    }
    return names;
}

/// Row `row` of the string column `name` (UTF-8 or a dictionary).
static std::optional<std::string> str_at(const tg::Blob &b, const char *name, std::uint32_t row) {
    const auto f = column(b, name);
    if (!f || row >= rows(b)) return std::nullopt;
    const std::uint32_t type = (*f)[0], data = (*f)[1], aux = (*f)[3];
    std::uint32_t at, end;
    std::size_t strings;
    if (type == 7) {
        at = u32_at(b, data + 4 * std::size_t{row});
        end = u32_at(b, data + 4 * (std::size_t{row} + 1));
        strings = aux;
    } else if (type == 11 || type == 12) {
        std::uint32_t code = 0;
        if (type == 11) {
            std::uint16_t c16;
            std::memcpy(&c16, b.data() + data + 2 * std::size_t{row}, 2);
            code = c16;
        } else {
            code = u32_at(b, data + 4 * std::size_t{row});
        }
        const std::uint32_t count = u32_at(b, aux);
        at = u32_at(b, aux + 4 + 4 * std::size_t{code});
        end = u32_at(b, aux + 4 + 4 * (std::size_t{code} + 1));
        strings = aux + 4 + 4 * (std::size_t{count} + 1);
    } else {
        return std::nullopt;
    }
    return std::string(reinterpret_cast<const char *>(b.data() + strings + at), end - at);
}

/// The string column `name`, every row.
static Strings strs(const tg::Blob &b, const char *name) {
    Strings out;
    for (std::uint32_t r = 0; r < rows(b); r++) out.push_back(str_at(b, name, r).value_or("\x01missing"));
    return out;
}

template <typename T>
static T load(const tg::Blob &b, std::size_t at) {
    T v;
    std::memcpy(&v, b.data() + at, sizeof v);
    return v;
}

/// Row `row` of the number column `name`, as a double.
static std::optional<double> num_at(const tg::Blob &b, const char *name, std::uint32_t row) {
    const auto f = column(b, name);
    if (!f || row >= rows(b)) return std::nullopt;
    const std::size_t data = (*f)[1], r = row;
    switch ((*f)[0]) {
    case 1: return load<std::int32_t>(b, data + 4 * r);
    case 2: return load<std::uint32_t>(b, data + 4 * r);
    case 3: return static_cast<double>(load<std::int64_t>(b, data + 8 * r));
    case 4: return static_cast<double>(load<std::uint64_t>(b, data + 8 * r));
    case 5: return load<float>(b, data + 4 * r);
    case 6: return load<double>(b, data + 8 * r);
    case 8: return load<std::uint8_t>(b, data + r);
    case 9: return load<std::int16_t>(b, data + 2 * r);
    case 10: return load<std::uint16_t>(b, data + 2 * r);
    default: return std::nullopt;
    }
}

static double gap(double a, double b) { return a > b ? a - b : b - a; }

/// Whether row `row` of column `name` is null, by this file's own read of the layout.
static bool null_at(const tg::Blob &b, const char *name, std::uint32_t row) {
    const auto f = column(b, name);
    return f && (*f)[5] != 0 && ((b.data()[(*f)[5] + row / 8] >> (row % 8)) & 1) == 0;
}

/// The Reader against this file's own reads of the layout (D-322); counts null values checked.
static void reader_matches(const tg::Blob &b, std::uint32_t &nulls) {
    const tg::Reader reader(b);
    CHECK(reader.rows() == rows(b) && reader.columns() == columns(b));
    const auto records = reader.records();
    CHECK(records.size() == reader.rows());
    for (const std::string &name : reader.columns()) {
        CHECK(reader.has(name));
        const auto values = reader.column(name);
        const auto is_null = reader.nulls(name);
        const bool text = str_at(b, name.c_str(), 0).has_value() || (rows(b) > 0 && !num_at(b, name.c_str(), 0));
        for (std::uint32_t r = 0; r < reader.rows(); r++) {
            const bool null = null_at(b, name.c_str(), r);
            if (null) nulls++;
            CHECK(is_null[r] == null && reader[r].is_null(name) == null);
            CHECK(std::holds_alternative<std::monostate>(values[r]) == null);
            CHECK(records[r].at(name) == values[r] && reader[r][name] == values[r]);
            if (null) continue;
            if (text) {
                const auto want = str_at(b, name.c_str(), r);
                CHECK(want && reader[r].string(name) == std::string_view(*want));
                CHECK(std::get<std::string_view>(values[r]) == *want);
            } else {
                const double want = num_at(b, name.c_str(), r).value_or(-1);
                CHECK(reader[r].number(name) == want && reader.numbers(name)[r] == want);
                const std::uint32_t type = (*column(b, name.c_str()))[0];
                const bool floating = type == 5 || type == 6;
                CHECK(std::holds_alternative<double>(values[r]) == floating);
                const double got = floating ? std::get<double>(values[r])
                                            : static_cast<double>(std::get<std::int64_t>(values[r]));
                CHECK(got == want);
                if (!floating) CHECK(reader[r].integer(name) == static_cast<std::int64_t>(want));
            }
        }
    }
    std::uint32_t seen = 0;
    for (auto row : reader) CHECK(row.index() == seen++);
    CHECK(seen == reader.rows() && !reader.has("nope"));
    CHECK(code_of([&] { (void)reader.numbers("nope"); }) == tg::ErrorCode::InvalidArgument);
    CHECK(code_of([&] { (void)tg::Reader(b.data(), b.size() - 8); }) == tg::ErrorCode::Corrupt);
}

static std::vector<std::uint8_t> read_file(const char *path) {
    std::ifstream in(path, std::ios::binary);
    return std::vector<std::uint8_t>(std::istreambuf_iterator<char>(in), {});
}

static void library() {
    CHECK(tg::version() == TGP_VERSION);
    CHECK(tg::components() == TGP_COMPONENTS);
    tg::noop();
    static const char *const fold[][2] = {TGP_FOLD_ITEMS};
    static_assert(sizeof fold / sizeof fold[0] == TGP_FOLD_COUNT, "fold count");
    for (const auto &pair : fold) CHECK(tg::fold(pair[0]) == pair[1]);
    CHECK(code_of([] { tg::fold("\xff"); }) == tg::ErrorCode::InvalidArgument);
    CHECK(message_of([] { tg::fold("\xff"); }).value_or("").size() > 0);

    tg::Blob sample = tg::sample_rows(TGP_SAMPLE_ROWS_ROWS);
    CHECK(sample.size() == TGP_SAMPLE_ROWS_BYTES && std::memcmp(sample.data(), "TGB1", 4) == 0);
    CHECK(rows(sample) == TGP_SAMPLE_ROWS_ROWS);
    CHECK(columns(sample) == Strings{TGP_SAMPLE_ROWS_COLUMNS_ITEMS});
    double lat = 0.0, elevation = 0.0;
    for (std::uint32_t r = 0; r < rows(sample); r++) { // in row order, as Rust sums
        lat += num_at(sample, "lat", r).value_or(0.0);
        elevation += num_at(sample, "elevation", r).value_or(0.0);
    }
    CHECK(lat == TGP_SAMPLE_ROWS_LAT_SUM && elevation == TGP_SAMPLE_ROWS_ELEVATION_SUM);
    CHECK(str_at(sample, "name", rows(sample) - 1) == TGP_SAMPLE_ROWS_LAST_NAME);
    tg::Blob moved = std::move(sample);
    CHECK(moved.size() == TGP_SAMPLE_ROWS_BYTES && sample.empty());
    CHECK(tg::SAMPLE_ROWS_MAX == TGP_SAMPLE_ROWS_MAX);
    CHECK(code_of([] { tg::sample_rows(tg::SAMPLE_ROWS_MAX + 1); }) == tg::ErrorCode::InvalidArgument);

    std::uint32_t sum = 0;
    CHECK(tg::progress(2, [&](std::uint32_t s) { sum += s; return s < 3; }) == 2 && sum == 3);
    const auto cancelled = [] {
        tg::progress(TGP_PROGRESS_STEPS, [](std::uint32_t s) { return s < TGP_PROGRESS_CANCEL_AFTER; });
    };
    CHECK(code_of(cancelled) == tg::ErrorCode::Cancelled);
    CHECK(message_of(cancelled) == TGP_PROGRESS_CANCELLED_MESSAGE);
    // A callback's exception cancels the call and comes out of it, not out of the library.
    bool rethrown = false;
    try {
        tg::progress(5, [](std::uint32_t s) -> bool {
            if (s == 2) throw std::logic_error("stop at 2");
            return true;
        });
    } catch (const std::logic_error &e) {
        rethrown = std::strcmp(e.what(), "stop at 2") == 0;
    }
    CHECK(rethrown);

    const double args[] = {TGP_DISTANCE_M_ARGS_ITEMS}, off[] = {TGP_DISTANCE_M_OUT_OF_RANGE_ITEMS};
    CHECK(gap(tg::distance_m(args[0], args[1], args[2], args[3]), TGP_DISTANCE_M_METRES) <= 1e-6);
    CHECK(code_of([&] { tg::distance_m(off[0], off[1], off[2], off[3]); }) == tg::ErrorCode::InvalidArgument);

    CHECK(static_cast<int>(tg::ErrorCode::InvalidArgument) == TGP_ERROR_CODES_INVALIDARGUMENT);
    CHECK(static_cast<int>(tg::ErrorCode::Cancelled) == TGP_ERROR_CODES_CANCELLED);
    CHECK(static_cast<int>(tg::ErrorCode::Internal) == TGP_ERROR_CODES_INTERNAL);
    CHECK(static_cast<int>(tg::ErrorCode::Io) == TGP_ERROR_CODES_IO);
    CHECK(static_cast<int>(tg::ErrorCode::Corrupt) == TGP_ERROR_CODES_CORRUPT);
    CHECK(static_cast<int>(tg::ErrorCode::MissingComponent) == TGP_ERROR_CODES_MISSINGCOMPONENT);
    CHECK(static_cast<int>(tg::ErrorCode::Network) == TGP_ERROR_CODES_NETWORK);
    CHECK(static_cast<int>(tg::ErrorCode::Refused) == TGP_ERROR_CODES_REFUSED);
}

static void engine_queries() {
    const char *path = FIXTURE(TGP_ENGINE_FIXTURE), *id = TGP_ENGINE_GET_ID;
    tg::Engine engine;
    CHECK(engine.open_file(path) == 0);
    tg::Blob got = engine.get(id);
    CHECK(rows(got) == 1 && columns(got) == Strings{TGP_ENGINE_COLUMNS_ITEMS});
    CHECK(str_at(got, "name", 0) == TGP_ENGINE_GET_NAME && str_at(got, "kind", 0) == TGP_ENGINE_GET_KIND);
    CHECK(num_at(got, "lat", 0) == TGP_ENGINE_GET_LAT && num_at(got, "lon", 0) == TGP_ENGINE_GET_LON);
    CHECK(str_at(engine.search(TGP_ENGINE_SEARCH_QUERY, TGP_ENGINE_SEARCH_LIMIT), "id", 0) == TGP_ENGINE_SEARCH_FIRST_ID);

    tg::Blob near = engine.reverse(TGP_ENGINE_REVERSE_LAT, TGP_ENGINE_REVERSE_LON, TGP_ENGINE_REVERSE_LIMIT);
    CHECK(strs(near, "id") == Strings{TGP_ENGINE_REVERSE_IDS_ITEMS});
    CHECK(gap(num_at(near, "meters", 0).value_or(0.0), TGP_ENGINE_REVERSE_FIRST_METERS) <= 0.1);
    std::uint32_t nulls = 0;
    reader_matches(near, nulls);
    reader_matches(engine.ancestors(TGP_ENGINE_ANCESTORS_ID), nulls);
    reader_matches(tg::sample_rows(20), nulls);
    CHECK(nulls > 0);
    CHECK(strs(engine.admin_at(TGP_ENGINE_ADMIN_AT_LAT, TGP_ENGINE_ADMIN_AT_LON), "id") == Strings{TGP_ENGINE_ADMIN_AT_IDS_ITEMS});
    CHECK(strs(engine.parent(TGP_ENGINE_PARENT_ID), "id") == Strings{TGP_ENGINE_PARENT_IDS_ITEMS});
    CHECK(strs(engine.ancestors(TGP_ENGINE_ANCESTORS_ID), "id") == Strings{TGP_ENGINE_ANCESTORS_IDS_ITEMS});
    CHECK(strs(engine.children(TGP_ENGINE_CHILDREN_ID), "id") == Strings{TGP_ENGINE_CHILDREN_IDS_ITEMS});
    CHECK(strs(engine.nearby(TGP_ENGINE_NEARBY_ID, TGP_ENGINE_NEARBY_LIMIT), "id") == Strings{TGP_ENGINE_NEARBY_IDS_ITEMS});
    CHECK(strs(engine.within_radius(TGP_ENGINE_WITHIN_RADIUS_LAT, TGP_ENGINE_WITHIN_RADIUS_LON, TGP_ENGINE_WITHIN_RADIUS_METERS,
                                    TGP_ENGINE_WITHIN_RADIUS_LIMIT),
               "id") == Strings{TGP_ENGINE_WITHIN_RADIUS_IDS_ITEMS});
    CHECK(code_of([&] { engine.within_radius(TGP_ENGINE_WITHIN_RADIUS_LAT, TGP_ENGINE_WITHIN_RADIUS_LON, -1.0, 1); }) ==
          tg::ErrorCode::InvalidArgument);
    CHECK(strs(engine.tz_at(TGP_ENGINE_TZ_AT_LAT, TGP_ENGINE_TZ_AT_LON), "tz") == Strings{TGP_ENGINE_TZ_AT_TZ});
    CHECK(strs(engine.by_iso(TGP_ENGINE_BY_ISO_CODE), "id") == Strings{TGP_ENGINE_BY_ISO_ID});
    CHECK(code_of([&] { engine.by_iso(TGP_ENGINE_BAD_ISO); }) == tg::ErrorCode::InvalidArgument);

    tg::SearchOptions focus;
    focus.focus_lat = TGP_ENGINE_SEARCH_WITH_FOCUS_LAT;
    focus.focus_lon = TGP_ENGINE_SEARCH_WITH_FOCUS_LON;
    focus.focus_scale_m = TGP_ENGINE_SEARCH_WITH_FOCUS_SCALE_M;
    const Strings focused{TGP_ENGINE_SEARCH_WITH_IDS_ITEMS};
    CHECK(strs(engine.search_with(TGP_ENGINE_SEARCH_WITH_QUERY, TGP_ENGINE_SEARCH_WITH_LIMIT, focus), "id") == focused);
    CHECK(strs(engine.search_with(TGP_ENGINE_SEARCH_WITH_QUERY, TGP_ENGINE_SEARCH_WITH_LIMIT), "id") != focused);
    tg::SearchOptions half;
    half.focus_lat = 1.0;
    CHECK(code_of([&] { engine.search_with(TGP_ENGINE_SEARCH_WITH_QUERY, 1, half); }) == tg::ErrorCode::InvalidArgument);

    tg::Blob postal = engine.postal_lookup(TGP_ENGINE_POSTAL_LOOKUP_CODE, TGP_ENGINE_POSTAL_LOOKUP_SCHEME);
    CHECK(strs(postal, "postal") == Strings{TGP_ENGINE_POSTAL_LOOKUP_POSTAL_ITEMS});
    CHECK(strs(postal, "id") == Strings{TGP_ENGINE_POSTAL_LOOKUP_IDS_ITEMS});
    CHECK(strs(engine.postal_prefix(TGP_ENGINE_POSTAL_PREFIX_INPUT, TGP_ENGINE_POSTAL_PREFIX_SCHEME, TGP_ENGINE_POSTAL_PREFIX_LIMIT),
               "postal") == Strings{TGP_ENGINE_POSTAL_PREFIX_POSTAL_ITEMS});
    tg::Blob at = engine.postal_at(TGP_ENGINE_POSTAL_AT_LAT, TGP_ENGINE_POSTAL_AT_LON);
    CHECK(strs(at, "postal") == Strings{TGP_ENGINE_POSTAL_AT_POSTAL_ITEMS});
    CHECK(strs(at, "code_kind") == Strings{TGP_ENGINE_POSTAL_AT_CODE_KIND_ITEMS});
    CHECK(strs(engine.postal_codes_of(TGP_ENGINE_POSTAL_CODES_OF_ID), "postal") == Strings{TGP_ENGINE_POSTAL_CODES_OF_POSTAL_ITEMS});

    CHECK(code_of([&] { engine.get(TGP_ENGINE_BAD_ID); }) == tg::ErrorCode::InvalidArgument);
    CHECK(code_of([&] { engine.open_file("/nonexistent.tgd"); }) == tg::ErrorCode::Io);
    CHECK(engine.layers() == 1);
    CHECK(engine.open_file(FIXTURE(TGP_ENGINE_ELEVATION_AT_FIXTURE)) == 1);
    tg::Blob height = engine.elevation_at(TGP_ENGINE_ELEVATION_AT_LAT, TGP_ENGINE_ELEVATION_AT_LON);
    CHECK(strs(height, "source") == Strings{TGP_ENGINE_ELEVATION_AT_SOURCE});
    CHECK(num_at(height, "meters", 0) == TGP_ENGINE_ELEVATION_AT_METERS);
    CHECK(engine.remove(1) && !engine.remove(1));

    tg::Engine taken = std::move(engine);
    CHECK(taken.native() != nullptr && engine.native() == nullptr);
    CHECK(rows(taken.get(id)) == 1);
}

static void open_options() {
    const char *path = FIXTURE(TGP_ENGINE_FIXTURE);
    std::vector<std::uint8_t> pack = read_file(path);
    CHECK(pack.size() > TGP_ENGINE_OPEN_WITH_CORRUPT_AT);
    if (pack.size() <= TGP_ENGINE_OPEN_WITH_CORRUPT_AT) return;
    pack[TGP_ENGINE_OPEN_WITH_CORRUPT_AT] ^= 0xff;
    tg::Engine scratch;
    tg::OpenOptions eager;
    eager.verify = tg::Verify::Eager;
    CHECK(code_of([&] { scratch.open_bytes_with(pack, eager); }) == tg::ErrorCode::Corrupt);
    CHECK(scratch.open_bytes_with(pack) == 0);
    tg::OpenOptions every;
    every.priority = 2;
    every.verify = tg::Verify::Eager;
    every.load = tg::Load::Read;
    every.outline_level = 1;
    CHECK(scratch.open_file_with(path, every) == 1);
    tg::OpenOptions odd;
    odd.verify = static_cast<tg::Verify>(TGP_ENGINE_OPEN_WITH_BAD_VERIFY);
    CHECK(code_of([&] { scratch.open_file_with(path, odd); }) == tg::ErrorCode::InvalidArgument);
    tg::OpenOptions deep;
    deep.outline_level = TGP_ENGINE_OPEN_WITH_BAD_OUTLINE_LEVEL;
    CHECK(code_of([&] { scratch.open_file_with(path, deep); }) == tg::ErrorCode::InvalidArgument);

    tg::OpenOptions points_only;
    points_only.components = TGP_ENGINE_OPEN_WITH_COMPONENTS;
    points_only.languages = TGP_ENGINE_OPEN_WITH_LANGUAGES;
    tg::Engine points;
    points.open_file_with(path, points_only);
    CHECK(rows(points.get(TGP_ENGINE_GET_ID)) == 1);
    CHECK(code_of([&] { points.search(TGP_ENGINE_SEARCH_QUERY, 1); }) == tg::ErrorCode::MissingComponent);
    tg::OpenOptions unknown;
    unknown.components = TGP_ENGINE_OPEN_WITH_BAD_COMPONENTS;
    CHECK(code_of([&] { points.open_file_with(path, unknown); }) == tg::ErrorCode::InvalidArgument);
}

static void zone_rules() {
    tg::Engine rules;
    CHECK(rules.open_file(FIXTURE(TGP_TZ_FIXTURE)) == 0);
    CHECK(rules.tz_version() == TGP_TZ_VERSION);
    // The same pack inside another file, opened from its range (an APK asset).
    tg::Engine embedded;
    const char *inside = FIXTURE(TGP_TZ_EMBEDDED_FIXTURE);
    embedded.open_file_range(inside, TGP_TZ_EMBEDDED_OFFSET, TGP_TZ_EMBEDDED_LENGTH);
    CHECK(embedded.tz_version() == TGP_TZ_VERSION);
    CHECK(code_of([&] { embedded.open_file_range(inside, TGP_TZ_EMBEDDED_OFFSET, TGP_TZ_EMBEDDED_PAST_END); }) == tg::ErrorCode::Io);
    CHECK(code_of([&] { embedded.open_file_range(inside, TGP_TZ_EMBEDDED_OFFSET - 1, TGP_TZ_EMBEDDED_LENGTH); }) == tg::ErrorCode::Corrupt);
    CHECK(strs(rules.tz_zones(), "zone") == Strings{TGP_TZ_ZONES_ITEMS});
    tg::Blob offset = rules.tz_offset_at(TGP_TZ_OFFSET_ZONE, TGP_TZ_OFFSET_UNIX);
    CHECK(num_at(offset, "seconds", 0) == TGP_TZ_OFFSET_SECONDS && num_at(offset, "dst", 0) == TGP_TZ_OFFSET_DST);
    CHECK(strs(offset, "abbr") == Strings{TGP_TZ_OFFSET_ABBR});
    const std::int64_t g[] = {TGP_TZ_GAP_CIVIL_ITEMS}, o[] = {TGP_TZ_OVERLAP_CIVIL_ITEMS};
    const auto local = [&](const char *zone, const std::int64_t *c) {
        return rules.tz_local(zone, static_cast<std::int32_t>(c[0]), static_cast<std::uint32_t>(c[1]),
                              static_cast<std::uint32_t>(c[2]), static_cast<std::uint32_t>(c[3]),
                              static_cast<std::uint32_t>(c[4]), static_cast<std::uint32_t>(c[5]));
    };
    tg::Blob in_gap = local(TGP_TZ_GAP_ZONE, g), in_overlap = local(TGP_TZ_OVERLAP_ZONE, o);
    CHECK(strs(in_gap, "kind") == Strings{"gap"} && strs(in_overlap, "kind") == Strings{"overlap"});
    CHECK(num_at(in_gap, "before", 0) == TGP_TZ_GAP_BEFORE && num_at(in_gap, "after", 0) == TGP_TZ_GAP_AFTER);
    CHECK(num_at(in_overlap, "before", 0) == TGP_TZ_OVERLAP_BEFORE && num_at(in_overlap, "after", 0) == TGP_TZ_OVERLAP_AFTER);
    CHECK(num_at(rules.tz_rules_start(TGP_TZ_START_ZONE), "unix", 0) == static_cast<double>(TGP_TZ_START_UNIX));
    CHECK(rows(rules.tz_offset_at(TGP_TZ_UNKNOWN, 0)) == 0);
    CHECK(code_of([&] { rules.tz_local(TGP_TZ_OFFSET_ZONE, 2024, 13, 1, 0, 0, 0); }) == tg::ErrorCode::InvalidArgument);
}

#if TEIGEODATA_FEATURE_STORE
/// The store: the fixture root, a mirror that refuses connections.
static void store_jobs() {
    const std::vector<std::uint8_t> root = read_file(FIXTURE(TGP_STORE_ROOT));
    const char *mirror = TGP_STORE_MIRROR, *pack = TGP_STORE_PACK;
    CHECK(code_of([] { tg::Store::open("target/cpp-store-missing"); }) == tg::ErrorCode::Refused);
    tg::Store store = tg::Store::create("target/cpp-store", mirror, mirror, root);
    tg::Engine installed = store.engine();
    CHECK(installed.layers() == 0);
    CHECK(rows(store.find(installed, pack, 3)) == 0);
    tg::InstallOptions wrong;
    wrong.components = TGP_STORE_BAD_COMPONENTS;
    CHECK(code_of([&] { store.install(pack, wrong); }) == tg::ErrorCode::InvalidArgument);
    tg::Job job = store.refresh(pack);
    CHECK(code_of([&] { job.result(); }) == tg::ErrorCode::Network);
    CHECK(job.is_ended() && job.done_bytes() == job.total_bytes());
    job.cancel();
}
#endif

int main() {
    library();
    engine_queries();
    open_options();
    zone_rules();
#if TEIGEODATA_FEATURE_STORE
    store_jobs();
#endif
    if (failures == 0) std::puts("c++ parity ok");
    return failures != 0;
}
