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
	"aarch64-apple-darwin":       "acb2124b728cf53796195d7b7b9bf5e6a91667569ed7b35d3ae176d5460f353b",
	"aarch64-apple-ios":          "c83c3ca3bdd08da54209c63b66d816a40b2ad5743b99c9ffc7a42ab3ccf3a85a",
	"aarch64-apple-ios-sim":      "173cae1a24ec7737210147fde21c3b4ea01d785bbf32ae7176841392a0aed796",
	"aarch64-linux-android":      "d62f70a80b440868d8f722102a76ddd6441827f1fac094dce3402f36e3f5980d",
	"aarch64-pc-windows-msvc":    "f88a610a9a642016b34299999564e8bf67a3167e04284c1f33a082818959a02a",
	"aarch64-unknown-linux-gnu":  "5a3270e5d4dc3ee4cbf7cf3a37741e52528d2c1d52cb58877a1019fcd4e18e88",
	"aarch64-unknown-linux-musl": "4af89dfcfa79c6bc7fb480c637d40ea3921c5d12c51e5c55a7c17b9299df545f",
	"armv7-linux-androideabi":    "c5f3c1c044e812bde63c1574695fc0ee07e23d7a89945defad59fc2a5f8b50ba",
	"x86_64-apple-darwin":        "cdf54918a26df283470e9c95e16a58fd90858a543629ff2d33a62b7d0c2fdeef",
	"x86_64-apple-ios":           "c4226bc007a0aaa143c6cf057d9bd9f4f51f2bb8303ed551fe08a43ee0ca996e",
	"x86_64-linux-android":       "06bbed8a0bed0d5f5c52167828b61c9a56a32ccc464c950f10a58b8ae26c93b4",
	"x86_64-pc-windows-msvc":     "9a02e498c38390ec1a421cbb42bd2cd465b5f5cc5ce69d9ddcfbf63893d22e97",
	"x86_64-unknown-linux-gnu":   "7faa776a97f41e5b2c314fceb3f46a1c61fe49a72b0e85bc397530424cad782b",
	"x86_64-unknown-linux-musl":  "052c7c02fb0bb8c76c8e3ccdf5de77b4be474642f11f2f1120ba081a1f9d9839",
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
