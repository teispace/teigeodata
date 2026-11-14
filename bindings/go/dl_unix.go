//go:build !cgo && !windows

package teigeodata

import "github.com/ebitengine/purego"

// open loads a library for the purego layer.
func open(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_LOCAL)
}

// symbol is the address of a library's function, or 0 when it has none.
func symbol(lib uintptr, name string) uintptr {
	addr, err := purego.Dlsym(lib, name)
	if err != nil {
		return 0
	}
	return addr
}
