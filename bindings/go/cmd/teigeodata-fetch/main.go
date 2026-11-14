// Command teigeodata-fetch downloads the release's prebuilt native library for this platform
// into the user cache, where package teigeodata finds it, and prints its path.
//
//	go run github.com/teispace/teigeodata/bindings/go/cmd/teigeodata-fetch [-base URL]
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"os/signal"

	"github.com/teispace/teigeodata/bindings/go/fetch"
)

func main() {
	base := flag.String("base", os.Getenv("TEIGEODATA_PREBUILT_BASE"),
		"download from this mirror of the release downloads (the pinned SHA-256 still applies)")
	flag.Parse()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	path, err := fetch.Library(ctx, *base)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(path)
}
