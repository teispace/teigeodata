// The tour: every query of the guide, run on a released pack.
//
// Usage: c++ -std=c++17 tour.cpp -I<include> -L<lib> -lteigeodata -o tour && ./tour world-np-lite.tgd

// #region import
#include <cstdio>
#include <iostream>
#include <string>

#include "teigeodata.hpp"
#include "teigeodata_reader.hpp"

namespace tg = teigeodata;
// #endregion import

/// `value` with `digits` decimals, as printf("%.*f") writes it.
static std::string fixed(std::optional<double> value, int digits) {
    char text[64];
    std::snprintf(text, sizeof text, "%.*f", digits, value.value_or(0.0));
    return text;
}

int main(int argc, char **argv) try {
    if (argc < 2) return 2;
    const std::string pack = argv[1];

    std::cout << "=== open\n";
    // #region open
    tg::Engine engine;
    engine.open_file(pack);
    std::cout << "layers: " << engine.layers() << '\n';
    // #endregion open

    std::cout << "=== search\n";
    // #region search
    const tg::Blob hits = engine.search("Pokhara", 3);
    for (auto p : tg::Reader(hits)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << "  "
                  << fixed(p.number("lat"), 4) << ", " << fixed(p.number("lon"), 4) << "  score "
                  << *p.integer("score") << '\n';
    }
    // #endregion search

    std::cout << "=== search-parent\n";
    // #region search-parent
    const tg::Blob parents = engine.search("Kathmandu, Bagmati", 2);
    for (auto p : tg::Reader(parents)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << ' '
                  << *p.integer("level") << '\n';
    }
    // #endregion search-parent

    std::cout << "=== search-focus\n";
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    tg::SearchOptions options;
    options.focus_lat = 28.7;
    options.focus_lon = 80.6;
    options.focus_scale_m = 30000;
    const tg::Blob near_focus = engine.search_with("Shivapur", 3, options);
    for (auto p : tg::Reader(near_focus)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << fixed(p.number("lat"), 4) << ", "
                  << fixed(p.number("lon"), 4) << '\n';
    }
    // #endregion search-focus

    std::cout << "=== get\n";
    // #region get
    const tg::Blob got = engine.get("tgd:np/1xhb");
    const tg::Reader one(got);
    const auto place = one[0];
    std::cout << *place.string("name") << ' ' << *place.string("kind") << '\n';
    std::cout << "at " << fixed(place.number("lat"), 6) << ", " << fixed(place.number("lon"), 6) << ", "
              << *place.integer("elevation") << " m\n";
    std::cout << "timezone " << *place.string("tz") << " parent " << *place.string("parent") << '\n';
    // #endregion get

    std::cout << "=== hierarchy\n";
    // #region hierarchy
    const tg::Blob ancestors = engine.ancestors("tgd:np/1xhb");
    for (auto p : tg::Reader(ancestors)) {
        std::cout << *p.string("kind") << ' ' << *p.integer("level") << "  " << *p.string("name");
        if (auto iso = p.string("iso")) std::cout << "  " << *iso;
        std::cout << '\n';
    }
    const tg::Blob children = engine.children("tgd:np/295c");
    std::cout << "Kaski has " << tg::Reader(children).rows() << " children\n";
    // #endregion hierarchy

    std::cout << "=== reverse\n";
    // #region reverse
    const tg::Blob around = engine.reverse(27.7172, 85.3240, 3);
    for (auto p : tg::Reader(around)) {
        std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m at "
                  << fixed(p.number("bearing"), 0) << "°\n";
    }
    // #endregion reverse

    std::cout << "=== radius\n";
    // #region radius
    const tg::Blob within = engine.within_radius(27.7172, 85.3240, 500, 10);
    const tg::Reader in_radius(within);
    std::cout << in_radius.rows() << " places within 500 m; farthest "
              << fixed(in_radius[in_radius.rows() - 1].number("meters"), 0) << " m\n";
    // #endregion radius

    std::cout << "=== nearby\n";
    // #region nearby
    const tg::Blob nearby = engine.nearby("tgd:np/2cn", 3);
    for (auto p : tg::Reader(nearby)) std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m\n";
    // #endregion nearby

    std::cout << "=== admin\n";
    // #region admin
    const tg::Blob units = engine.admin_at(28.2096, 83.9856);
    for (auto p : tg::Reader(units)) {
        std::cout << *p.integer("level") << "  " << *p.string("name");
        if (auto iso = p.string("iso")) std::cout << "  " << *iso;
        std::cout << '\n';
    }
    const tg::Blob state = engine.by_iso("NP-P4");
    const auto province = tg::Reader(state)[0];
    std::cout << "NP-P4 is " << *province.string("name") << ' ' << *province.string("id") << '\n';
    // #endregion admin

    std::cout << "=== timezone\n";
    // #region timezone
    const tg::Blob area = engine.tz_at(27.7172, 85.3240);
    const std::string zone(*tg::Reader(area)[0].string("tz"));
    const tg::Blob at = engine.tz_offset_at(zone, 1700000000);
    const auto offset = tg::Reader(at)[0];
    std::cout << zone << ' ' << *offset.integer("seconds") << " s east of UTC, " << *offset.string("abbr") << " dst "
              << *offset.integer("dst") << '\n';
    const tg::Blob civil = engine.tz_local("America/New_York", 2026, 3, 8, 2, 30, 0);
    const auto local = tg::Reader(civil)[0];
    std::cout << "02:30 on 2026-03-08 in New York: " << *local.string("kind") << ' ' << *local.integer("before")
              << " -> " << *local.integer("after") << '\n';
    std::cout << "tzdb " << engine.tz_version() << '\n';
    // #endregion timezone

    std::cout << "=== elevation\n";
    // #region elevation
    const tg::Blob ground = engine.elevation_at(27.7172, 85.3240);
    const auto height = tg::Reader(ground)[0];
    std::cout << fixed(height.number("meters"), 1) << " m above " << *height.string("datum") << " ("
              << *height.string("source") << ", " << fixed(height.number("resolution_m"), 0) << " m grid)\n";
    // #endregion elevation

    std::cout << "=== distance\n";
    // #region distance
    const double meters = tg::distance_m(27.7172, 85.3240, 28.2096, 83.9856);
    const double heading = tg::bearing_deg(27.7172, 85.3240, 28.2096, 83.9856);
    std::cout << "Kathmandu to Pokhara: " << fixed(meters / 1000, 1) << " km, heading " << fixed(heading, 0) << "°\n";
    const tg::Blob between = engine.distance("tgd:np/2cn", "tgd:np/1xhb");
    const auto apart = tg::Reader(between)[0];
    std::cout << "place to place: " << fixed(*apart.number("meters") / 1000, 1) << " km at "
              << fixed(apart.number("bearing"), 0) << "°\n";
    // #endregion distance

    std::cout << "=== boundary\n";
    // #region boundary
    const tg::Blob outline = engine.boundary("tgd:np/2gm", 0);
    const auto shape = tg::Reader(outline)[0];
    const std::string_view geojson = *shape.string("geojson"); // a GeoJSON Feature, for any map library
    const std::string_view key = "\"geometry\":{\"type\":\"";
    const std::string_view type = geojson.substr(geojson.find(key) + key.size());
    std::cout << type.substr(0, type.find('"')) << " of " << *shape.integer("vertices") << " vertices at level "
              << *shape.integer("lod") << '\n';
    std::cout << "tolerance " << fixed(shape.number("tolerance_m"), 0) << " m, " << geojson.size() << " bytes of GeoJSON\n";
    // #endregion boundary

    std::cout << "=== errors\n";
    // #region errors
    try {
        engine.reverse(91, 0, 1);
    } catch (const tg::Error &error) {
        if (error.code() == tg::ErrorCode::InvalidArgument) std::cout << "invalid argument: " << error.what() << '\n';
    }
    // #endregion errors
    return 0;
} catch (const tg::Error &error) {
    std::cerr << "error " << static_cast<int>(error.code()) << ": " << error.what() << '\n';
    return 1;
}
