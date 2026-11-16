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
const Version = "1.1.0"

// Base is where releases are downloaded from: Base + "/v" + Version + "/" + Asset(triple).
const Base = "https://github.com/teispace/teigeodata/releases/download"

// SHA256 is each triple's library file, lowercase hex. The release pipeline writes it when it
// uploads the libraries (phase 5.10); until the first such release it is empty, and a library
// comes from TEIGEODATA_LIB, LoadLibrary or the system's library path.
var SHA256 = map[string]string{
	"aarch64-apple-darwin":       "f5ad93c7b5f6730896c5eacb2b65c793ddb601c8e7bc049ae45128f495322037",
	"aarch64-apple-ios":          "aacd1639aa0d8b3c62b980ee7ac93e82890310c06f916c0e4cccc80ef28d3f31",
	"aarch64-apple-ios-sim":      "a45a076481b3fe5ce766bd454666be9a480f4e68ae0455aa25f14a12beea7806",
	"aarch64-linux-android":      "ca5563e1014c8c3a274636a76ad945b07a01a220ae991e86ea9f0e32852e2837",
	"aarch64-pc-windows-msvc":    "6df883025b00afdf2ab565092d4bfaba276eb78a65ecf0163da722d05a662026",
	"aarch64-unknown-linux-gnu":  "cd32850a17a6a3f2b9290caaabb2df99e9e57df1efc62a11a5e5757f19fb15e9",
	"aarch64-unknown-linux-musl": "d5196838f55a53c503d0853d4a30f1858600088d19ac2929a65055a273235a27",
	"armv7-linux-androideabi":    "4894534908be7ff483848b657a2e0529c5808df0ff1688162a17c05c69e5ede9",
	"x86_64-apple-darwin":        "cd1cb615648b9ac43e99fbcbcb887f1d20ee46ed4c611faddafd96663b29f09a",
	"x86_64-apple-ios":           "751cc604c073e99f0257964c5b39f36ac540e74109673f06d0351317b321ff80",
	"x86_64-linux-android":       "f10fbf64eecbab9c3c25f32edbcd66fd707911d1405424a4ad45dfb800facb4a",
	"x86_64-pc-windows-msvc":     "e92ecf66c28413cee104b0e742a07d48b390a6cc93d9bdddea46799c091ca0c7",
	"x86_64-unknown-linux-gnu":   "2564e9d07c191de04562f511a3dab3402b3d442a5df11e904d7a9b73fc6b0881",
	"x86_64-unknown-linux-musl":  "487fcec8b24e6cf615151810b8511f29b6c23646da349684b5e0a30f733a6654",
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
