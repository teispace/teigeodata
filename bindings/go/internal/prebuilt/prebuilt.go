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
	"aarch64-apple-darwin":       "8676869a5bcbf1031ebe00dcf6bc37e276d9c8dae9757ac49db14262e02967d6",
	"aarch64-apple-ios":          "f8c71b8d0825b6dac883adeede02878b8201657f49699c378b23aefba7d5e1a2",
	"aarch64-apple-ios-sim":      "97de8c575ea0f279791755e16f1dd8c03318dd84db42e017db22363a05d085ef",
	"aarch64-linux-android":      "28d15346fed29f0cb136d3691dd20473f4ef78ce60e0064b62fc5dd6510cb385",
	"aarch64-pc-windows-msvc":    "4148c56a03f8df6bff55909c8e47ed3bb2c18eee8091e7da9ec96be36f488736",
	"aarch64-unknown-linux-gnu":  "12670693d2be69bebe351c83f86f1eb670c7f962f748531cfc34cc7c2ab02126",
	"aarch64-unknown-linux-musl": "61369bcbec467fda0ebfea8d11df3b5fe14e4d8d92c7f6d7434358c05c6456a0",
	"armv7-linux-androideabi":    "fd5e3646d39546ac2222a060a13429db95466da283344ec7906b064c90a43021",
	"x86_64-apple-darwin":        "4cf7ea84d1d1c38f402a782c07cc08786d9a7395400ed25b4ec6658efa82abb2",
	"x86_64-apple-ios":           "ef001f4d58052e1dd419995c5152ee3fa55cbde9dab3682cd7289fff417f0aab",
	"x86_64-linux-android":       "7cd597353c47c0ad9e8d9048276d1797569b1aff45896955f8c292ff24ddacda",
	"x86_64-pc-windows-msvc":     "99ac8a4704b24a580e595bd489627ca4f12c7af33b71a839cc54d08d15912034",
	"x86_64-unknown-linux-gnu":   "a8b140074797c28a12b99e5071fcf0b6cb5f98a90dc2977a56ff4f7e0dbc7427",
	"x86_64-unknown-linux-musl":  "2e4fe04dae382ff21e722730f2a558990cf537b9761f492e85da850db0648948",
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
