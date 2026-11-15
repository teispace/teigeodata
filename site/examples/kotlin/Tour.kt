// The tour: every query of the guide, run on a released pack.
//
// Usage: kotlinc Tour.kt -cp <the package's classes> -include-runtime -d tour.jar
//        java --enable-native-access=ALL-UNNAMED -cp tour.jar:<the package's classes> TourKt world-np-lite.tgd

// #region import
import java.util.Locale
import org.teispace.teigeodata.Reader
import org.teispace.teigeodata.TeiGeodata
import org.teispace.teigeodata.TeiGeodata.Engine
import org.teispace.teigeodata.TeiGeodata.SearchOptions
import org.teispace.teigeodata.TeiGeodataException
// #endregion import

/** `format` in the root locale: a decimal point whatever the user's locale. */
fun String.f(vararg args: Any?): String = String.format(Locale.ROOT, this, *args)

fun main(args: Array<String>) {
    val pack = args[0]

    println("=== open")
    // #region open
    Engine().use { engine ->
        engine.openFile(pack)
        println("layers: ${engine.layers()}")
        // #endregion open
        tour(engine)
    }
}

fun tour(engine: Engine) {
    println("=== search")
    // #region search
    for (p in Reader(engine.search("Pokhara", 3))) {
        println("%s  %s  %s  %.4f, %.4f  score %d".f(p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"]))
    }
    // #endregion search

    println("=== search-parent")
    // #region search-parent
    for (p in Reader(engine.search("Kathmandu, Bagmati", 2))) {
        println("${p["id"]}  ${p["name"]}  ${p["kind"]} ${p["level"]}")
    }
    // #endregion search-parent

    println("=== search-focus")
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    val options = SearchOptions(28.7, 80.6, 30_000.0, false)
    for (p in Reader(engine.searchWith("Shivapur", 3, options))) {
        println("%s  %s  %.4f, %.4f".f(p["id"], p["name"], p["lat"], p["lon"]))
    }
    // #endregion search-focus

    println("=== get")
    // #region get
    val place = Reader(engine.get("tgd:np/1xhb")).single()
    println("${place["name"]} ${place["kind"]}")
    println("at %.6f, %.6f, %d m".f(place["lat"], place["lon"], place["elevation"]))
    println("timezone ${place["tz"]} parent ${place["parent"]}")
    // #endregion get

    println("=== hierarchy")
    // #region hierarchy
    for (p in Reader(engine.ancestors("tgd:np/1xhb"))) {
        println("${p["kind"]} ${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
    }
    val children = Reader(engine.children("tgd:np/295c"))
    println("Kaski has ${children.rows()} children")
    // #endregion hierarchy

    println("=== reverse")
    // #region reverse
    for (p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
        println("%s  %.0f m at %.0f°".f(p["name"], p["meters"], p["bearing"]))
    }
    // #endregion reverse

    println("=== radius")
    // #region radius
    val within = Reader(engine.withinRadius(27.7172, 85.3240, 500.0, 10)).records()
    println("%d places within 500 m; farthest %.0f m".f(within.size, within.last()["meters"]))
    // #endregion radius

    println("=== nearby")
    // #region nearby
    for (p in Reader(engine.nearby("tgd:np/2cn", 3))) {
        println("%s  %.0f m".f(p["name"], p["meters"]))
    }
    // #endregion nearby

    println("=== admin")
    // #region admin
    for (p in Reader(engine.adminAt(28.2096, 83.9856))) {
        println("${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
    }
    val state = Reader(engine.byIso("NP-P4")).single()
    println("NP-P4 is ${state["name"]} ${state["id"]}")
    // #endregion admin

    println("=== timezone")
    // #region timezone
    val zone = Reader(engine.tzAt(27.7172, 85.3240)).single()["tz"] as String
    val offset = Reader(engine.tzOffsetAt(zone, 1_700_000_000)).single()
    println("$zone ${offset["seconds"]} s east of UTC, ${offset["abbr"]} dst ${offset["dst"]}")
    val local = Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).single()
    println("02:30 on 2026-03-08 in New York: ${local["kind"]} ${local["before"]} -> ${local["after"]}")
    println("tzdb ${engine.tzVersion()}")
    // #endregion timezone

    println("=== elevation")
    // #region elevation
    val height = Reader(engine.elevationAt(27.7172, 85.3240)).single()
    println("%.1f m above %s (%s, %.0f m grid)".f(height["meters"], height["datum"], height["source"], height["resolution_m"]))
    // #endregion elevation

    println("=== distance")
    // #region distance
    val meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856)
    val heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856)
    println("Kathmandu to Pokhara: %.1f km, heading %.0f°".f(meters / 1000, heading))
    val apart = Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).single()
    println("place to place: %.1f km at %.0f°".f(apart["meters"] as Double / 1000, apart["bearing"]))
    // #endregion distance

    println("=== boundary")
    // #region boundary
    val shape = Reader(engine.boundary("tgd:np/2gm", 0)).single()
    val geojson = shape["geojson"] as String // a GeoJSON Feature, for any map library
    val type = geojson.substringAfter("\"geometry\":{\"type\":\"").substringBefore('"')
    println("%s of %s vertices at level %s".f(type, shape["vertices"], shape["lod"]))
    println("tolerance %.0f m, %d bytes of GeoJSON".f(shape["tolerance_m"], geojson.length))
    // #endregion boundary

    println("=== errors")
    // #region errors
    try {
        engine.reverse(91.0, 0.0, 1)
    } catch (error: TeiGeodataException.InvalidArgument) {
        println("invalid argument: ${error.message}")
    }
    // #endregion errors
}
