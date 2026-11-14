// Package teigeodata is the Go binding of teigeodata: places, admin areas, elevation, postal
// codes and time zones from signed offline packs, through the native library's C ABI.
//
// # The native library
//
// The binding calls the C package's shared library (libteigeodata), which it loads at run
// time, so a Go program links nothing and cross-compiles with cgo or without it. The library is
// the first of: the file passed to LoadLibrary; the file TEIGEODATA_LIB names; the release's prebuilt
// library in the user cache, fetched once by package fetch or by
//
//	go run github.com/teispace/teigeodata/bindings/go/cmd/teigeodata-fetch
//
// and checked against the SHA-256 this module pins; and the system's library search path (an
// installed C package). It must be this module's version.
//
// With cgo (the default where a C compiler is present) each call costs what a cgo call costs,
// about 25 ns. Built with CGO_ENABLED=0 the same API runs on purego, at 5 to 10 times that on
// small calls.
//
// # Results
//
// Lists come back as a blob, a []byte of typed columns read with Reader: numbers in place,
// strings once each. A failure is an *Error whose Code is the same in every language:
//
//	engine, err := teigeodata.NewEngine()
//	defer engine.Close()
//	_, err = engine.OpenFile("np.tgd")
//	blob, err := engine.Search("Kathmandu", 5)
//	r, err := teigeodata.NewReader(blob)
//	names, err := r.Strings("name")
//	if errors.Is(err, teigeodata.CodeMissingComponent) { ... }
//
// Handles (Engine, Store, Job) may be used from any goroutine at once. Close frees one at
// once; otherwise it is freed when unreachable.
package teigeodata
