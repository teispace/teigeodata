//go:build !cgo && windows

package teigeodata

import "syscall"

// open loads a library for the purego layer.
func open(path string) (uintptr, error) {
	handle, err := syscall.LoadLibrary(path)
	return uintptr(handle), err
}

// symbol is the address of a library's function, or 0 when it has none.
func symbol(lib uintptr, name string) uintptr {
	addr, err := syscall.GetProcAddress(syscall.Handle(lib), name)
	if err != nil {
		return 0
	}
	return addr
}
