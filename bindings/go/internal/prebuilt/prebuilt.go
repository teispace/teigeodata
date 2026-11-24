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
const Version = "1.3.0"

// Base is where releases are downloaded from: Base + "/v" + Version + "/" + Asset(triple).
const Base = "https://github.com/teispace/teigeodata/releases/download"

// SHA256 is each triple's library file, lowercase hex. The release pipeline writes it when it
// uploads the libraries (phase 5.10); until the first such release it is empty, and a library
// comes from TEIGEODATA_LIB, LoadLibrary or the system's library path.
var SHA256 = map[string]string{
	"aarch64-apple-darwin":       "4cd3482939845cd341d09b5f64d87744e01d7809e72749d8bcdf15432f779fd9",
	"aarch64-apple-ios":          "fc94ef918cdf2a22884f6276719feff1d40602c82279d19cccd5db7cab4024e3",
	"aarch64-apple-ios-sim":      "c1cb4508b9ac429f7a5adc155800ae7ad4c1feb427d21fcf1ed699479db7a7c4",
	"aarch64-linux-android":      "dc4c8886d0b8beb6e3be393310714f4695a98a2e4252925af341e2278a847161",
	"aarch64-pc-windows-msvc":    "f57141539126af9e01bfb0f6601136ce0b70130f99e77036013d036cd6d9cc6f",
	"aarch64-unknown-linux-gnu":  "7b6756e1bbc7236533c54eea15a2abd7b55958cda0633d019c3663eae20fc4ae",
	"aarch64-unknown-linux-musl": "001e6d7a951204dea074f5baf6d79dc3a579f7f1382f0c76baa6771cf72b0e0c",
	"armv7-linux-androideabi":    "2f243b656008eb60685ac78fbdaa42d7883520eac4b37a69a692ba2c094a2869",
	"x86_64-apple-darwin":        "672597b8e20342a68b712934e43a2cbb97f6f32c59c214195e6cd9ecb2de7642",
	"x86_64-apple-ios":           "7208c3d720f3e9396e7ca8cfeb322ee18aaa98f3a7c92088523a7c5fc3d9ed6b",
	"x86_64-linux-android":       "924132a64215896dc842c6cddea85cec8cfac925d9a693a01f2ac5ce4220531e",
	"x86_64-pc-windows-msvc":     "63b0a3e10d807e276e886cb052a3dcfee0e45a31fb2be1b8f80b3d12aa5adb63",
	"x86_64-unknown-linux-gnu":   "c3fa92540c0803ae7217e675783b66b45f9c2f2c97d979ba0fff87cab89d0f64",
	"x86_64-unknown-linux-musl":  "e6eefb57807d027a789bf1dd8895ef96081491942a5f5589006579b0fee456aa",
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
