package teigeodata

import (
	"runtime"
	"strconv"
	"sync"
	"unsafe"
)

// tgBytes is the C tg_bytes: a buffer the library allocated, freed by tg_bytes_free. Its
// pointer is a uintptr: it is never Go memory, and a type without pointers costs cgo no check.
type tgBytes struct {
	ptr uintptr
	len uintptr
	cap uintptr
}

// bytes views the buffer, valid until it is freed.
func (b *tgBytes) bytes() []byte {
	return unsafe.Slice(*(**byte)(unsafe.Pointer(&b.ptr)), b.len)
}

// Error is a failed call: its stable code and the library's message. Test for a code with
// errors.Is(err, teigeodata.CodeIo), or read it with errors.As.
type Error struct {
	Code    ErrorCode
	Message string
}

func (e *Error) Error() string { return "teigeodata: " + e.Code.String() + ": " + e.Message }

// Is reports whether target is this error's code, so that errors.Is(err, CodeCancelled) holds.
func (e *Error) Is(target error) bool {
	code, ok := target.(ErrorCode)
	return ok && code == e.Code
}

// Error makes a code usable as an errors.Is target.
func (c ErrorCode) Error() string { return "teigeodata: " + c.String() }

// newError is the failure a raw call returned: -1 when the library lacks the function (a
// feature it was built without), else the code and message it gave.
func newError(status int32, msg *tgBytes, symbol string) error {
	if status == -1 {
		return &Error{
			Code:    CodeMissingComponent,
			Message: "the loaded library has no " + symbol + ": it was built without that feature",
		}
	}
	return &Error{Code: ErrorCode(status), Message: takeString(*msg)}
}

// takeString copies a library string into Go memory and frees it.
func takeString(b tgBytes) string {
	if b.ptr == 0 {
		return ""
	}
	s := string(b.bytes())
	rawBytesFree(b)
	return s
}

// takeBytes copies a library blob into Go memory and frees it. Go memory, not a view of the
// library's: a column slice of it then keeps it alive, as the garbage collector cannot do for
// C memory (D-297). The copy is 8-byte aligned, so Values views its columns in place.
func takeBytes(b tgBytes) []byte {
	if b.ptr == 0 {
		return []byte{}
	}
	out := append([]byte(nil), b.bytes()...)
	rawBytesFree(b)
	return out
}

// pinString is a string's bytes for a C struct, pinned until the call returns (cgo forbids an
// unpinned Go pointer inside memory passed to C).
func pinString(pinner *runtime.Pinner, s string) (unsafe.Pointer, uintptr) {
	if s == "" {
		return nil, 0
	}
	data := unsafe.StringData(s)
	pinner.Pin(data)
	return unsafe.Pointer(data), uintptr(len(s))
}

func itoa(v int64) string { return strconv.FormatInt(v, 10) }

// A callback crosses C as a context number naming its Go function here: a Go pointer may not
// be kept by C, and a number survives both cgo and purego.
type callbackEntry struct {
	fn       any
	panicked any
}

var callbacks = struct {
	sync.Mutex
	next uintptr
	live map[uintptr]*callbackEntry
}{live: map[uintptr]*callbackEntry{}}

func registerCallback(fn any) uintptr {
	callbacks.Lock()
	defer callbacks.Unlock()
	callbacks.next++
	if callbacks.next == 0 {
		callbacks.next = 1
	}
	callbacks.live[callbacks.next] = &callbackEntry{fn: fn}
	return callbacks.next
}

func lookupCallback(ctx uintptr) *callbackEntry {
	callbacks.Lock()
	defer callbacks.Unlock()
	return callbacks.live[ctx]
}

// unregisterCallback forgets ctx and returns the panic its function raised, if any.
func unregisterCallback(ctx uintptr) any {
	callbacks.Lock()
	defer callbacks.Unlock()
	entry := callbacks.live[ctx]
	delete(callbacks.live, ctx)
	if entry == nil {
		return nil
	}
	return entry.panicked
}
