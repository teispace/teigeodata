// The tour: every query of the guide, run on a released pack.
//
// Usage: go run . world-np-lite.tgd
package main

// #region import
import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"strings"

	tg "github.com/teispace/teigeodata/bindings/go"
)

// #endregion import

// #region rows
// rows reads a result: every row as a Record (column name to value, nil where null).
func rows(blob []byte, err error) []tg.Record {
	if err != nil {
		log.Fatal(err)
	}
	r, err := tg.NewReader(blob)
	if err != nil {
		log.Fatal(err)
	}
	records, err := r.Records()
	if err != nil {
		log.Fatal(err)
	}
	return records
}

// #endregion rows

// orEmpty is a nullable string column's value, "" where null.
func orEmpty(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func main() {
	pack := os.Args[1]

	fmt.Println("=== open")
	// #region open
	engine, err := tg.NewEngine()
	if err != nil {
		log.Fatal(err)
	}
	defer engine.Close()
	if _, err := engine.OpenFile(pack); err != nil {
		log.Fatal(err)
	}
	fmt.Println("layers:", engine.Layers())
	// #endregion open

	fmt.Println("=== search")
	// #region search
	for _, p := range rows(engine.Search("Pokhara", 3)) {
		fmt.Printf("%s  %s  %s  %.4f, %.4f  score %d\n",
			p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"])
	}
	// #endregion search

	fmt.Println("=== search-parent")
	// #region search-parent
	for _, p := range rows(engine.Search("Kathmandu, Bagmati", 2)) {
		fmt.Printf("%s  %s  %s %d\n", p["id"], p["name"], p["kind"], p["level"])
	}
	// #endregion search-parent

	fmt.Println("=== search-focus")
	// #region search-focus
	// Many places share a name; a focus (the user's location) ranks the near ones first.
	lat, lon, scale := 28.7, 80.6, 30_000.0
	options := &tg.SearchOptions{FocusLat: &lat, FocusLon: &lon, FocusScaleM: &scale}
	for _, p := range rows(engine.SearchWith("Shivapur", 3, options)) {
		fmt.Printf("%s  %s  %.4f, %.4f\n", p["id"], p["name"], p["lat"], p["lon"])
	}
	// #endregion search-focus

	fmt.Println("=== get")
	// #region get
	place := rows(engine.Get("tgd:np/1xhb"))[0]
	fmt.Println(place["name"], place["kind"])
	fmt.Printf("at %.6f, %.6f, %d m\n", place["lat"], place["lon"], place["elevation"])
	fmt.Println("timezone", place["tz"], "parent", place["parent"])
	// #endregion get

	fmt.Println("=== hierarchy")
	// #region hierarchy
	for _, p := range rows(engine.Ancestors("tgd:np/1xhb")) {
		line := fmt.Sprintf("%s %d  %s  %s", p["kind"], p["level"], p["name"], orEmpty(p["iso"]))
		fmt.Println(strings.TrimRight(line, " "))
	}
	children := rows(engine.Children("tgd:np/295c"))
	fmt.Println("Kaski has", len(children), "children")
	// #endregion hierarchy

	fmt.Println("=== reverse")
	// #region reverse
	for _, p := range rows(engine.Reverse(27.7172, 85.3240, 3)) {
		fmt.Printf("%s  %.0f m at %.0f°\n", p["name"], p["meters"], p["bearing"])
	}
	// #endregion reverse

	fmt.Println("=== radius")
	// #region radius
	within := rows(engine.WithinRadius(27.7172, 85.3240, 500, 10))
	fmt.Printf("%d places within 500 m; farthest %.0f m\n", len(within), within[len(within)-1]["meters"])
	// #endregion radius

	fmt.Println("=== nearby")
	// #region nearby
	for _, p := range rows(engine.Nearby("tgd:np/2cn", 3)) {
		fmt.Printf("%s  %.0f m\n", p["name"], p["meters"])
	}
	// #endregion nearby

	fmt.Println("=== admin")
	// #region admin
	for _, p := range rows(engine.AdminAt(28.2096, 83.9856)) {
		fmt.Println(strings.TrimRight(fmt.Sprintf("%d  %s  %s", p["level"], p["name"], orEmpty(p["iso"])), " "))
	}
	state := rows(engine.ByIso("NP-P4"))[0]
	fmt.Println("NP-P4 is", state["name"], state["id"])
	// #endregion admin

	fmt.Println("=== timezone")
	// #region timezone
	zone := rows(engine.TzAt(27.7172, 85.3240))[0]["tz"].(string)
	offset := rows(engine.TzOffsetAt(zone, 1_700_000_000))[0]
	fmt.Println(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
	local := rows(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0))[0]
	fmt.Println("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
	fmt.Println("tzdb", engine.TzVersion())
	// #endregion timezone

	fmt.Println("=== elevation")
	// #region elevation
	height := rows(engine.ElevationAt(27.7172, 85.3240))[0]
	fmt.Printf("%.1f m above %s (%s, %.0f m grid)\n",
		height["meters"], height["datum"], height["source"], height["resolution_m"])
	// #endregion elevation

	fmt.Println("=== distance")
	// #region distance
	meters, err := tg.DistanceM(27.7172, 85.3240, 28.2096, 83.9856)
	if err != nil {
		log.Fatal(err)
	}
	heading, err := tg.BearingDeg(27.7172, 85.3240, 28.2096, 83.9856)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("Kathmandu to Pokhara: %.1f km, heading %.0f°\n", meters/1000, heading)
	apart := rows(engine.Distance("tgd:np/2cn", "tgd:np/1xhb"))[0]
	fmt.Printf("place to place: %.1f km at %.0f°\n", apart["meters"].(float64)/1000, apart["bearing"])
	// #endregion distance

	fmt.Println("=== boundary")
	// #region boundary
	shape := rows(engine.Boundary("tgd:np/2gm", 0))[0]
	geojson := shape["geojson"].(string) // a GeoJSON Feature, for any map library
	var feature struct {
		Geometry struct{ Type string } `json:"geometry"`
	}
	if err := json.Unmarshal([]byte(geojson), &feature); err != nil {
		log.Fatal(err)
	}
	fmt.Printf("%s of %d vertices at level %d\n", feature.Geometry.Type, shape["vertices"], shape["lod"])
	fmt.Printf("tolerance %.0f m, %d bytes of GeoJSON\n", shape["tolerance_m"], len(geojson))
	// #endregion boundary

	fmt.Println("=== errors")
	// #region errors
	_, err = engine.Reverse(91, 0, 1)
	var failure *tg.Error
	if errors.As(err, &failure) && failure.Code == tg.CodeInvalidArgument {
		fmt.Println("invalid argument:", failure.Message)
	}
	// #endregion errors
}
