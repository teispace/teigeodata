package teigeodata

import (
	"errors"
	"os"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/teispace/teigeodata/bindings/go/internal/prebuilt"
)

// The native library, loaded once per process: by LoadLibrary, or on the first call from
//
//  1. the file TEIGEODATA_LIB names;
//  2. the prebuilt library in the user cache that package fetch filled, if its SHA-256 is
//     the one pinned for this version;
//  3. the system's library search path (an installed C package, D-286).
//
// Each must be this module's version: another is refused.
var library struct {
	sync.Mutex
	ready atomic.Bool
	path  string
	err   error
}

// LoadLibrary loads the native library at path for every later call. Without it, the first call
// finds the library itself (see the package documentation). Loading another path once a
// library is loaded is refused.
func LoadLibrary(path string) error {
	library.Lock()
	defer library.Unlock()
	if library.ready.Load() {
		if path == library.path {
			return nil
		}
		return errors.New("teigeodata: " + library.path + " is already loaded")
	}
	if err := loadFrom(path); err != nil {
		return err
	}
	library.err = nil
	return nil
}

// LibraryPath is the file the native library was loaded from, or "" before it is.
func LibraryPath() string {
	if !library.ready.Load() {
		return ""
	}
	library.Lock()
	defer library.Unlock()
	return library.path
}

// ensure loads the library on first use; afterwards it costs one atomic load.
func ensure() error {
	if library.ready.Load() {
		return nil
	}
	return loadDefault()
}

func loadDefault() error {
	library.Lock()
	defer library.Unlock()
	if library.ready.Load() {
		return nil
	}
	if library.err != nil {
		return library.err
	}
	library.err = findAndLoad()
	return library.err
}

func findAndLoad() error {
	if path := os.Getenv("TEIGEODATA_LIB"); path != "" {
		return loadFrom(path)
	}
	var tried []string
	if triple, err := prebuilt.Triple(); err != nil {
		tried = append(tried, err.Error())
	} else if want := prebuilt.SHA256[triple]; want != "" {
		path, err := prebuilt.CachePath(triple)
		if err == nil {
			got, herr := prebuilt.FileSHA256(path)
			switch {
			case herr != nil:
				tried = append(tried, path+": not fetched (go run github.com/teispace/teigeodata/bindings/go/cmd/teigeodata-fetch)")
			case got != want:
				tried = append(tried, path+": SHA-256 "+got+", not the pinned "+want)
			default:
				return loadFrom(path)
			}
		}
	}
	err := loadFrom(prebuilt.FileName())
	if err == nil {
		return nil
	}
	tried = append(tried, err.Error())
	return errors.New("teigeodata: no native library found; set TEIGEODATA_LIB to its file, " +
		"call teigeodata.LoadLibrary, or fetch the prebuilt one with package fetch. Tried: " +
		strings.Join(tried, "; "))
}

// loadFrom loads the library at path and checks its version. Called with library held.
func loadFrom(path string) error {
	if err := rawLoad(path); err != nil {
		return errors.New("teigeodata: " + path + ": " + err.Error())
	}
	var out, msg tgBytes
	if status := rawVersion(&out, &msg); status != 0 {
		return newError(status, &msg, "tg_version")
	}
	if got := takeString(out); got != prebuilt.Version {
		return errors.New("teigeodata: " + path + " is version " + got + "; this module calls " +
			prebuilt.Version)
	}
	library.path = path
	library.ready.Store(true)
	return nil
}
