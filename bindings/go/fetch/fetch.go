// Package fetch downloads the release's prebuilt native library for this platform into the
// user cache, where package teigeodata's loader finds it. It is a separate package so that a
// program that never downloads links no HTTP client.
package fetch

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"

	"github.com/teispace/teigeodata/bindings/go/internal/prebuilt"
)

// Library makes sure this platform's prebuilt library is in the user cache, downloading it
// from base (the release's GitHub downloads when "") if it is missing or does not match its
// pinned SHA-256, and returns its path. A file whose SHA-256 differs is never kept.
func Library(ctx context.Context, base string) (string, error) {
	triple, err := prebuilt.Triple()
	if err != nil {
		return "", err
	}
	want := prebuilt.SHA256[triple]
	if want == "" {
		return "", fmt.Errorf("teigeodata %s has no prebuilt library for %s; set TEIGEODATA_LIB "+
			"to a library built from source (cargo build -p teigeodata-c --profile dist)",
			prebuilt.Version, triple)
	}
	path, err := prebuilt.CachePath(triple)
	if err != nil {
		return "", err
	}
	if got, err := prebuilt.FileSHA256(path); err == nil && got == want {
		return path, nil
	}
	if base == "" {
		base = prebuilt.Base
	}
	url := base + "/v" + prebuilt.Version + "/" + prebuilt.Asset(triple)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return "", err
	}
	part := path + "." + strconv.Itoa(os.Getpid()) + ".part"
	defer os.Remove(part)
	if err := download(ctx, url, part); err != nil {
		return "", err
	}
	got, err := prebuilt.FileSHA256(part)
	if err != nil {
		return "", err
	}
	if got != want {
		return "", fmt.Errorf("teigeodata: %s has SHA-256 %s, not the pinned %s; not used", url, got, want)
	}
	// Moved into place whole, so a concurrent loader never reads half a file.
	if err := os.Rename(part, path); err != nil {
		return "", err
	}
	return path, nil
}

func download(ctx context.Context, url, to string) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return err
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return errors.New("teigeodata: HTTP " + resp.Status + " for " + url)
	}
	f, err := os.Create(to)
	if err != nil {
		return err
	}
	if _, err := io.Copy(f, resp.Body); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}
