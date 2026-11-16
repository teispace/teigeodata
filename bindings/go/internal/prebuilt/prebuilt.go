// Package prebuilt names the release's prebuilt libraries: the version this module calls, each
// platform's Rust triple and file, where a fetched library is cached, and its pinned SHA-256.
// The loader reads the cache; package fetch fills it.
package prebuilt

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
)

// Version is the library version this module's generated code calls. The loader refuses a
// library of another version.
const Version = "1.0.1"

// Base is where releases are downloaded from: Base + "/v" + Version + "/" + Asset(triple).
const Base = "https://github.com/teispace/teigeodata/releases/download"

// SHA256 is each triple's library file, lowercase hex. The release pipeline writes it when it
// uploads the libraries (phase 5.10); until the first such release it is empty, and a library
// comes from TEIGEODATA_LIB, LoadLibrary or the system's library path.
var SHA256 = map[string]string{
	"aarch64-apple-darwin":       "e6aa531f7d8941a9d217d5ec8363ec1343947f6006143ae205a001bf9b910af6",
	"aarch64-apple-ios":          "6619989f5da08b5d5fd2636c0a4b381c1b044e2a3ddc965483d94b03c13c162e",
	"aarch64-apple-ios-sim":      "30579803f5e392fb654d064137cb1eeb93d3e9c6685a69984c20e1e697c75883",
	"aarch64-linux-android":      "3de78bdb3524d397c5c37c327becafd80bd773c631b2a9f108cca2f33439eb2b",
	"aarch64-pc-windows-msvc":    "7a6f881fc712bc7cd7a84aa60027dd7ccab8c148937e77b5278f74f56fd7d709",
	"aarch64-unknown-linux-gnu":  "dc729dc9b0eadf1790133148dedc91f1e2abcfb7c8b291f4cfa1aba6f6daf02d",
	"aarch64-unknown-linux-musl": "baf9166a0f019141dd240284b31c158057b1b3c868a472991d3db2fd66b3ef9c",
	"armv7-linux-androideabi":    "38a6f767367c1c27f19cd6192fb4aa79bf45991e3bd2073f9f60d3edbe4767b8",
	"x86_64-apple-darwin":        "8af815b15818adb04d7456376c1b44fd5a2ac28fd268f24f0033cc44026ea8a9",
	"x86_64-apple-ios":           "91e95fae5e753dbc12cc61c1552a93d9c2e6c8938f0558a2b90629cdd93d84da",
	"x86_64-linux-android":       "190af92c1d7fcad17ec065a9182e98acc2063da56a35fe68517ee274ab8d670e",
	"x86_64-pc-windows-msvc":     "b9eb65f1661d38bb99c6cca1e15ddc95a67c47f3cca5d9b391e286221d2314e5",
	"x86_64-unknown-linux-gnu":   "df9fdd88384fe33c19a0bfc1b7984794c2e5c9a36d3517f49263ccb26a72f95f",
	"x86_64-unknown-linux-musl":  "5ae30bb65a62468082fe4b404728842c0ea96ee5cfe94005c6a51372177e65b5",
}

// Triple is this platform's Rust target triple.
func Triple() (string, error) {
	arch := map[string]string{"amd64": "x86_64", "arm64": "aarch64"}[runtime.GOARCH]
	if arch == "" {
		return "", errors.New("teigeodata: no library for " + runtime.GOOS + "/" + runtime.GOARCH)
	}
	switch runtime.GOOS {
	case "darwin":
		return arch + "-apple-darwin", nil
	case "linux":
		if musl() {
			return arch + "-unknown-linux-musl", nil
		}
		return arch + "-unknown-linux-gnu", nil
	case "windows":
		return arch + "-pc-windows-msvc", nil
	}
	return "", errors.New("teigeodata: no library for " + runtime.GOOS + "/" + runtime.GOARCH)
}

// musl reports whether this Linux runs musl: its dynamic loader is /lib/ld-musl-<arch>.so.1.
func musl() bool {
	found, _ := filepath.Glob("/lib/ld-musl-*.so.1")
	return len(found) > 0
}

// FileName is the library's file name on this platform.
func FileName() string {
	switch runtime.GOOS {
	case "windows":
		return "teigeodata.dll"
	case "darwin":
		return "libteigeodata.dylib"
	}
	return "libteigeodata.so"
}

// Asset is the release asset holding triple's library (the Dart hook's name, D-289).
func Asset(triple string) string { return "teigeodata-" + Version + "-" + triple + "-" + FileName() }

// CachePath is where a fetched library for triple is kept: the user's cache directory, then
// teigeodata/<version>/<triple>/<file>.
func CachePath(triple string) (string, error) {
	dir, err := os.UserCacheDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "teigeodata", Version, triple, FileName()), nil
}

// FileSHA256 is the SHA-256 of a file, lowercase hex.
func FileSHA256(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
