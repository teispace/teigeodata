// The tour: every query of the guide, run on a released pack.
//
// Usage: java --enable-native-access=ALL-UNNAMED -cp <the package's classes> Tour.java world-np-lite.tgd

// #region import
import java.util.List;
import java.util.Locale;
import java.util.Map;
import org.teispace.teigeodata.Reader;
import org.teispace.teigeodata.TeiGeodata;
import org.teispace.teigeodata.TeiGeodata.Engine;
import org.teispace.teigeodata.TeiGeodata.SearchOptions;
import org.teispace.teigeodata.TeiGeodataException;
// #endregion import

public final class Tour {
    /** {@code String.format} in the root locale: a decimal point whatever the user's locale. */
    static String f(String format, Object... args) {
        return String.format(Locale.ROOT, format, args);
    }

    /** {@code value}, or "" where it is null. */
    static Object orEmpty(Object value) {
        return value == null ? "" : value;
    }

    public static void main(String[] args) {
        String pack = args[0];

        System.out.println("=== open");
        // #region open
        try (Engine engine = new Engine()) {
            engine.openFile(pack);
            System.out.println("layers: " + engine.layers());
            // #endregion open
            tour(engine);
        }
    }

    static void tour(Engine engine) {
        System.out.println("=== search");
        // #region search
        for (Map<String, Object> p : new Reader(engine.search("Pokhara", 3))) {
            System.out.println(f("%s  %s  %s  %.4f, %.4f  score %d",
                    p.get("id"), p.get("name"), p.get("kind"), p.get("lat"), p.get("lon"), p.get("score")));
        }
        // #endregion search

        System.out.println("=== search-parent");
        // #region search-parent
        for (Map<String, Object> p : new Reader(engine.search("Kathmandu, Bagmati", 2))) {
            System.out.println(f("%s  %s  %s %d", p.get("id"), p.get("name"), p.get("kind"), p.get("level")));
        }
        // #endregion search-parent

        System.out.println("=== search-focus");
        // #region search-focus
        // Many places share a name; a focus (the user's location) ranks the near ones first.
        SearchOptions options = SearchOptions.DEFAULTS.withFocusLat(28.7).withFocusLon(80.6).withFocusScaleM(30_000.0);
        for (Map<String, Object> p : new Reader(engine.searchWith("Shivapur", 3, options))) {
            System.out.println(f("%s  %s  %.4f, %.4f", p.get("id"), p.get("name"), p.get("lat"), p.get("lon")));
        }
        // #endregion search-focus

        System.out.println("=== get");
        // #region get
        Map<String, Object> place = new Reader(engine.get("tgd:np/1xhb")).records().get(0);
        System.out.println(place.get("name") + " " + place.get("kind"));
        System.out.println(f("at %.6f, %.6f, %d m", place.get("lat"), place.get("lon"), place.get("elevation")));
        System.out.println("timezone " + place.get("tz") + " parent " + place.get("parent"));
        // #endregion get

        System.out.println("=== hierarchy");
        // #region hierarchy
        for (Map<String, Object> p : new Reader(engine.ancestors("tgd:np/1xhb"))) {
            System.out.println(f("%s %d  %s  %s", p.get("kind"), p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
        }
        Reader children = new Reader(engine.children("tgd:np/295c"));
        System.out.println("Kaski has " + children.rows() + " children");
        // #endregion hierarchy

        System.out.println("=== reverse");
        // #region reverse
        for (Map<String, Object> p : new Reader(engine.reverse(27.7172, 85.3240, 3))) {
            System.out.println(f("%s  %.0f m at %.0f°", p.get("name"), p.get("meters"), p.get("bearing")));
        }
        // #endregion reverse

        System.out.println("=== radius");
        // #region radius
        List<Map<String, Object>> within = new Reader(engine.withinRadius(27.7172, 85.3240, 500, 10)).records();
        System.out.println(f("%d places within 500 m; farthest %.0f m",
                within.size(), within.get(within.size() - 1).get("meters")));
        // #endregion radius

        System.out.println("=== nearby");
        // #region nearby
        for (Map<String, Object> p : new Reader(engine.nearby("tgd:np/2cn", 3))) {
            System.out.println(f("%s  %.0f m", p.get("name"), p.get("meters")));
        }
        // #endregion nearby

        System.out.println("=== admin");
        // #region admin
        for (Map<String, Object> p : new Reader(engine.adminAt(28.2096, 83.9856))) {
            System.out.println(f("%d  %s  %s", p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
        }
        Map<String, Object> state = new Reader(engine.byIso("NP-P4")).records().get(0);
        System.out.println("NP-P4 is " + state.get("name") + " " + state.get("id"));
        // #endregion admin

        System.out.println("=== timezone");
        // #region timezone
        String zone = (String) new Reader(engine.tzAt(27.7172, 85.3240)).records().get(0).get("tz");
        Map<String, Object> offset = new Reader(engine.tzOffsetAt(zone, 1_700_000_000L)).records().get(0);
        System.out.println(zone + " " + offset.get("seconds") + " s east of UTC, " + offset.get("abbr") + " dst " + offset.get("dst"));
        Map<String, Object> local = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records().get(0);
        System.out.println("02:30 on 2026-03-08 in New York: " + local.get("kind") + " " + local.get("before") + " -> " + local.get("after"));
        System.out.println("tzdb " + engine.tzVersion());
        // #endregion timezone

        System.out.println("=== elevation");
        // #region elevation
        Map<String, Object> height = new Reader(engine.elevationAt(27.7172, 85.3240)).records().get(0);
        System.out.println(f("%.1f m above %s (%s, %.0f m grid)",
                height.get("meters"), height.get("datum"), height.get("source"), height.get("resolution_m")));
        // #endregion elevation

        System.out.println("=== distance");
        // #region distance
        double meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856);
        double heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
        System.out.println(f("Kathmandu to Pokhara: %.1f km, heading %.0f°", meters / 1000, heading));
        Map<String, Object> apart = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records().get(0);
        System.out.println(f("place to place: %.1f km at %.0f°", (Double) apart.get("meters") / 1000, apart.get("bearing")));
        // #endregion distance

        System.out.println("=== boundary");
        // #region boundary
        Map<String, Object> shape = new Reader(engine.boundary("tgd:np/2gm", 0)).records().get(0);
        String geojson = (String) shape.get("geojson"); // a GeoJSON Feature, for any map library
        int at = geojson.indexOf("\"type\":\"", geojson.indexOf("\"geometry\"")) + 8;
        String type = geojson.substring(at, geojson.indexOf('"', at));
        System.out.println(f("%s of %s vertices at level %s", type, shape.get("vertices"), shape.get("lod")));
        System.out.println(f("tolerance %.0f m, %d bytes of GeoJSON", shape.get("tolerance_m"), geojson.length()));
        // #endregion boundary

        System.out.println("=== errors");
        // #region errors
        try {
            engine.reverse(91, 0, 1);
        } catch (TeiGeodataException.InvalidArgument error) {
            System.out.println("invalid argument: " + error.getMessage());
        }
        // #endregion errors
    }
}
