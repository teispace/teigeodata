/* The C ABI from C++ through the generated wrapper (teigeodata.hpp, 5.8d): the same calls as
 * parity.c, with what the wrapper adds checked too: exceptions with their codes, a callback's
 * exception carried across the library, records with their defaults, move-only handles.
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

#include "teigeodata.hpp"

namespace tg = teigeodata;

static int failures = 0;
#define CHECK(cond)                                                                  \
    do {                                                                             \
        if (!(cond)) {                                                               \
            std::fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                              \
        }                                                                            \
    } while (0)

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

static std::uint32_t u32_at(const tg::Blob &b, std::size_t at) {
    std::uint32_t v;
    std::memcpy(&v, b.data() + at, 4);
    return v;
}

static std::uint32_t rows(const tg::Blob &b) { return u32_at(b, 4); }

/// Row `row` of the string column `name` (UTF-8 or a dictionary) of a v1 blob.
static std::optional<std::string> str_at(const tg::Blob &b, const char *name, std::uint32_t row) {
    const std::uint32_t columns = u32_at(b, 8);
    for (std::uint32_t i = 0; i < columns && row < rows(b); i++) {
        const std::size_t d = 16 + 40 * i;
        if (std::strncmp(reinterpret_cast<const char *>(b.data() + d), name, 16) != 0) continue;
        std::uint32_t f[6];
        std::memcpy(f, b.data() + d + 16, sizeof f);
        std::uint32_t at, end;
        std::size_t strings;
        if (f[0] == 7) {
            at = u32_at(b, f[1] + 4 * row);
            end = u32_at(b, f[1] + 4 * (row + 1));
            strings = f[3];
        } else if (f[0] == 11 || f[0] == 12) {
            std::uint32_t code = 0;
            if (f[0] == 11) {
                std::uint16_t c16;
                std::memcpy(&c16, b.data() + f[1] + 2 * row, 2);
                code = c16;
            } else {
                code = u32_at(b, f[1] + 4 * row);
            }
            const std::uint32_t count = u32_at(b, f[3]);
            at = u32_at(b, f[3] + 4 + 4 * code);
            end = u32_at(b, f[3] + 4 + 4 * (code + 1));
            strings = f[3] + 4 + 4 * (count + 1);
        } else {
            return std::nullopt;
        }
        return std::string(reinterpret_cast<const char *>(b.data() + strings + at), end - at);
    }
    return std::nullopt;
}

static std::vector<std::uint8_t> read_file(const char *path) {
    std::ifstream in(path, std::ios::binary);
    return std::vector<std::uint8_t>(std::istreambuf_iterator<char>(in), {});
}

int main() {
    CHECK(tg::version() == "0.3.1");
    CHECK(tg::components().rfind("point, elevation, admin", 0) == 0);
    tg::noop();
    CHECK(tg::fold("S\xc3\xa3o  Paulo") == "sao paulo");
    CHECK(code_of([] { tg::fold("\xff"); }) == tg::ErrorCode::InvalidArgument);
    try {
        tg::fold("\xff");
    } catch (const tg::Error &e) {
        CHECK(std::strlen(e.what()) > 0);
    }

    tg::Blob sample = tg::sample_rows(1000);
    CHECK(sample.size() == 37120 && std::memcmp(sample.data(), "TGB1", 4) == 0);
    tg::Blob moved = std::move(sample);
    CHECK(moved.size() == 37120 && sample.empty());
    CHECK(code_of([] { tg::sample_rows(tg::SAMPLE_ROWS_MAX + 1); }) == tg::ErrorCode::InvalidArgument);

    std::uint32_t sum = 0;
    CHECK(tg::progress(2, [&](std::uint32_t s) { sum += s; return s < 3; }) == 2 && sum == 3);
    CHECK(code_of([] { tg::progress(9, [](std::uint32_t s) { return s < 3; }); }) == tg::ErrorCode::Cancelled);
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

    // The engine on the fixture pack (bindings/parity.json, "engine").
    tg::Engine engine;
    const char *path = "bindings/fixtures/synth.tgd", *id = "tgd:np/18n8a6";
    CHECK(engine.open_file(path) == 0);
    tg::Blob got = engine.get(id);
    CHECK(rows(got) == 1 && str_at(got, "name", 0) == "Synthland" && str_at(got, "kind", 0) == "country");
    CHECK(str_at(engine.search("Synthland", 3), "id", 0) == id);
    tg::Blob near = engine.reverse(27.5, 85.5, 3);
    CHECK(rows(near) == 3 && str_at(near, "id", 0) == "tgd:np/sx2ze9");
    CHECK(str_at(engine.admin_at(27.5, 85.5), "id", 1) == "tgd:np/avwsr1");
    CHECK(rows(engine.children(id)) == 8);
    CHECK(code_of([&] { engine.within_radius(27.5, 85.5, -1.0, 10); }) == tg::ErrorCode::InvalidArgument);
    CHECK(str_at(engine.tz_at(27.5, 85.5), "tz", 0) == "Zone 1");

    tg::SearchOptions focus;
    focus.focus_lat = 26.80005;
    focus.focus_lon = 87.087849;
    focus.focus_scale_m = 1000.0;
    CHECK(str_at(engine.search_with("Lalek", 3, focus), "id", 1) == "tgd:np/tv88y0");
    CHECK(str_at(engine.search_with("Lalek", 3), "id", 1) != "tgd:np/tv88y0");
    tg::SearchOptions half;
    half.focus_lat = 1.0;
    CHECK(code_of([&] { engine.search_with("Lalek", 3, half); }) == tg::ErrorCode::InvalidArgument);

    // Open options: the byte at 173392 (parity.json, open_with) lies inside a section.
    std::vector<std::uint8_t> pack = read_file(path);
    CHECK(pack.size() > 173392);
    pack[173392] ^= 0xff;
    tg::Engine scratch;
    tg::OpenOptions eager;
    eager.verify = tg::Verify::Eager;
    CHECK(code_of([&] { scratch.open_bytes_with(pack, eager); }) == tg::ErrorCode::Corrupt);
    CHECK(scratch.open_bytes_with(pack) == 0);
    tg::OpenOptions points_only;
    points_only.components = "point";
    points_only.languages = "en, ne";
    tg::Engine points;
    points.open_file_with(path, points_only);
    CHECK(rows(points.get(id)) == 1);
    CHECK(code_of([&] { points.search("Synthland", 1); }) == tg::ErrorCode::MissingComponent);

    tg::Blob postal = engine.postal_lookup("106-01", "np");
    CHECK(str_at(postal, "postal", 0) == "10601" && str_at(postal, "id", 0) == "tgd:np/vasbex");
    CHECK(code_of([&] { engine.get("not an id"); }) == tg::ErrorCode::InvalidArgument);
    CHECK(code_of([&] { engine.open_file("/nonexistent.tgd"); }) == tg::ErrorCode::Io);
    CHECK(engine.open_file("bindings/fixtures/dem.tgd") == 1);
    CHECK(str_at(engine.elevation_at(27.84, 76.01), "source", 0) == "copernicus-glo30");
    CHECK(engine.remove(1) && !engine.remove(1));

    tg::Engine taken = std::move(engine);
    CHECK(taken.native() != nullptr && engine.native() == nullptr);
    CHECK(rows(taken.get(id)) == 1);

    // The zone rules (parity.json, tz) on the rules-only fixture.
    tg::Engine rules;
    rules.open_file("bindings/fixtures/tz.tgd");
    CHECK(rules.tz_version() == "2026f");
    CHECK(str_at(rules.tz_offset_at("America/New_York", 1719835200), "abbr", 0) == "EDT");
    CHECK(str_at(rules.tz_local("America/New_York", 2024, 3, 10, 2, 30, 0), "kind", 0) == "gap");

#if TEIGEODATA_FEATURE_STORE
    // The store (parity.json, store): the fixture root, a mirror that refuses connections.
    const std::vector<std::uint8_t> root = read_file("bindings/fixtures/store-root.json");
    const char *mirror = "http://127.0.0.1:1/";
    CHECK(code_of([] { tg::Store::open("target/cpp-store-missing"); }) == tg::ErrorCode::Refused);
    tg::Store store = tg::Store::create("target/cpp-store", mirror, mirror, root);
    tg::Engine installed = store.engine();
    CHECK(installed.layers() == 0);
    CHECK(rows(store.find(installed, "np", 3)) == 0);
    tg::Job job = store.refresh("np");
    CHECK(code_of([&] { job.result(); }) == tg::ErrorCode::Network);
    CHECK(job.is_ended() && job.done_bytes() == job.total_bytes());
    job.cancel();
#endif

    if (failures == 0) std::puts("c++ parity ok");
    return failures != 0;
}
