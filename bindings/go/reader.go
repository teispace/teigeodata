package teigeodata

import (
	"encoding/binary"
	"errors"
	"unsafe"
)

// ColumnType is the type of a blob column (format v1, D-275).
type ColumnType uint32

// The column types of format v1.
const (
	TypeI32    ColumnType = 1
	TypeU32    ColumnType = 2
	TypeI64    ColumnType = 3
	TypeU64    ColumnType = 4
	TypeF32    ColumnType = 5
	TypeF64    ColumnType = 6
	TypeUtf8   ColumnType = 7
	TypeU8     ColumnType = 8
	TypeI16    ColumnType = 9
	TypeU16    ColumnType = 10
	TypeDict16 ColumnType = 11
	TypeDict32 ColumnType = 12
)

// width is the bytes per row of a column's data: an offset for Utf8, a code for a dictionary.
func (t ColumnType) width() int {
	switch t {
	case TypeU8:
		return 1
	case TypeI16, TypeU16, TypeDict16:
		return 2
	case TypeI32, TypeU32, TypeF32, TypeUtf8, TypeDict32:
		return 4
	case TypeI64, TypeU64, TypeF64:
		return 8
	}
	return 0
}

const (
	header     = 16
	descriptor = 40
	nameMax    = 16
)

// ErrMalformed is returned for bytes that are not a well-formed blob.
var ErrMalformed = errors.New("teigeodata: malformed blob")

type column struct {
	name           string
	ty             ColumnType
	at, n          int
	auxAt, auxLen  int
	nullsAt, nullN int
}

// Reader reads a result blob (format v1, or v0) in place: its rows and named columns. Every
// blob result of this package is one; NewReader checks the layout, and each column is checked
// again when it is read. Number columns are views of the blob, strings are copied when read
// (a dictionary's strings once each).
//
//	blob, err := engine.Search("Kathmandu", 5)
//	r, err := teigeodata.NewReader(blob)
//	names, err := r.Strings("name")
//	lats, err := teigeodata.Values[float64](r, "lat")
type Reader struct {
	data    []byte
	rows    int
	columns []column
}

// NewReader checks data's header and column descriptors and returns its reader.
func NewReader(data []byte) (*Reader, error) {
	if len(data) < header {
		return nil, ErrMalformed
	}
	magic := string(data[:4])
	v0 := magic == "TGB0"
	if !v0 && magic != "TGB1" {
		return nil, ErrMalformed
	}
	rows := int(u32(data, 4))
	count := int(u32(data, 8))
	if int(u32(data, 12)) != len(data) || count > (len(data)-header)/descriptor {
		return nil, ErrMalformed
	}
	floor := header + count*descriptor
	r := &Reader{data: data, rows: rows, columns: make([]column, count)}
	for i := range count {
		d := data[header+i*descriptor : header+(i+1)*descriptor]
		end := 0
		for end < nameMax && d[end] != 0 {
			end++
		}
		for _, b := range d[end:nameMax] {
			if b != 0 {
				return nil, ErrMalformed
			}
		}
		c := column{
			name:    string(d[:end]),
			ty:      ColumnType(u32(d, nameMax)),
			at:      int(u32(d, nameMax+4)),
			n:       int(u32(d, nameMax+8)),
			auxAt:   int(u32(d, nameMax+12)),
			auxLen:  int(u32(d, nameMax+16)),
			nullsAt: int(u32(d, nameMax+20)),
		}
		width := c.ty.width()
		values := rows
		if c.ty == TypeUtf8 {
			values++
		}
		hasAux := c.ty == TypeUtf8 || c.ty == TypeDict16 || c.ty == TypeDict32
		switch {
		case end == 0, width == 0, r.index(c.name) >= 0:
		case v0 && (c.nullsAt != 0 || c.ty > TypeUtf8):
		case c.n != values*width, !region(data, c.at, c.n, floor):
		case hasAux && !region(data, c.auxAt, c.auxLen, floor):
		case !hasAux && (c.auxAt != 0 || c.auxLen != 0):
		case c.nullsAt != 0 && !region(data, c.nullsAt, (rows+7)/8, floor):
		default:
			if c.nullsAt != 0 {
				c.nullN = (rows + 7) / 8
			}
			r.columns[i] = c
			continue
		}
		return nil, ErrMalformed
	}
	return r, nil
}

// region reports whether [at, at+n) lies after the descriptors and inside data, 8-aligned.
func region(data []byte, at, n, floor int) bool {
	return at >= floor && at%8 == 0 && n >= 0 && at <= len(data) && n <= len(data)-at
}

func u32(b []byte, at int) uint32 { return binary.LittleEndian.Uint32(b[at:]) }

func (r *Reader) index(name string) int {
	for i, c := range r.columns {
		if c.name == name {
			return i
		}
	}
	return -1
}

// Rows is the number of rows.
func (r *Reader) Rows() int { return r.rows }

// Has reports whether there is a column called name.
func (r *Reader) Has(name string) bool { return r.index(name) >= 0 }

// Columns are the column names, in order.
func (r *Reader) Columns() []string {
	names := make([]string, len(r.columns))
	for i, c := range r.columns {
		names[i] = c.name
	}
	return names
}

// Type is the type of the column called name, and whether there is one.
func (r *Reader) Type(name string) (ColumnType, bool) {
	if i := r.index(name); i >= 0 {
		return r.columns[i].ty, true
	}
	return 0, false
}

func (r *Reader) column(name string) (column, error) {
	if i := r.index(name); i >= 0 {
		return r.columns[i], nil
	}
	return column{}, errors.New("teigeodata: the blob has no column " + name)
}

// Nulls is, per row, whether the column called name is null there; nil when no row is.
func (r *Reader) Nulls(name string) ([]bool, error) {
	c, err := r.column(name)
	if err != nil || c.nullsAt == 0 {
		return nil, err
	}
	bits := r.data[c.nullsAt : c.nullsAt+c.nullN]
	nulls := make([]bool, r.rows)
	for i := range nulls {
		nulls[i] = bits[i/8]>>(i%8)&1 == 0
	}
	return nulls, nil
}

// Number is a type a number column holds.
type Number interface {
	int32 | uint32 | int64 | uint64 | float32 | float64 | uint8 | int16 | uint16
}

func typeOf[T Number]() ColumnType {
	var zero T
	switch any(zero).(type) {
	case int32:
		return TypeI32
	case uint32:
		return TypeU32
	case int64:
		return TypeI64
	case uint64:
		return TypeU64
	case float32:
		return TypeF32
	case float64:
		return TypeF64
	case uint8:
		return TypeU8
	case int16:
		return TypeI16
	}
	return TypeU16
}

var littleEndian = func() bool {
	x := uint16(1)
	return *(*byte)(unsafe.Pointer(&x)) == 1
}()

// Values is the number column called name, which must hold T (a null row reads as zero: see
// Nulls). It is a view of the blob, without a copy, where the blob is aligned and the machine
// little-endian, and a copy otherwise; either way it must not be written to.
func Values[T Number](r *Reader, name string) ([]T, error) {
	c, err := r.column(name)
	if err != nil {
		return nil, err
	}
	if c.ty != typeOf[T]() {
		return nil, errors.New("teigeodata: column " + name + " does not hold that type")
	}
	if r.rows == 0 {
		return []T{}, nil
	}
	data := r.data[c.at : c.at+c.n]
	var zero T
	size := int(unsafe.Sizeof(zero))
	if littleEndian && uintptr(unsafe.Pointer(&data[0]))%uintptr(size) == 0 {
		return unsafe.Slice((*T)(unsafe.Pointer(&data[0])), r.rows), nil
	}
	out := make([]T, r.rows)
	for i := range out {
		at := data[i*size:]
		switch size {
		case 1:
			*(*uint8)(unsafe.Pointer(&out[i])) = at[0]
		case 2:
			*(*uint16)(unsafe.Pointer(&out[i])) = binary.LittleEndian.Uint16(at)
		case 4:
			*(*uint32)(unsafe.Pointer(&out[i])) = binary.LittleEndian.Uint32(at)
		default:
			*(*uint64)(unsafe.Pointer(&out[i])) = binary.LittleEndian.Uint64(at)
		}
	}
	return out, nil
}

// Strings is the string column called name (UTF-8 or a dictionary); a null row reads as ""
// (see Nulls).
func (r *Reader) Strings(name string) ([]string, error) {
	c, err := r.column(name)
	if err != nil {
		return nil, err
	}
	data := r.data[c.at : c.at+c.n]
	aux := r.data[c.auxAt : c.auxAt+c.auxLen]
	out := make([]string, r.rows)
	switch c.ty {
	case TypeUtf8:
		for i := range out {
			s, ok := slice(aux, data, i)
			if !ok {
				return nil, ErrMalformed
			}
			out[i] = s
		}
	case TypeDict16, TypeDict32:
		if len(aux) < 8 {
			return nil, ErrMalformed
		}
		count := int(u32(aux, 0))
		if count > (len(aux)-8)/4 {
			return nil, ErrMalformed
		}
		offsets, strs := aux[4:8+4*count], aux[8+4*count:]
		words := make([]string, count)
		done := make([]bool, count)
		for i := range out {
			var code int
			if c.ty == TypeDict16 {
				code = int(binary.LittleEndian.Uint16(data[2*i:]))
			} else {
				code = int(u32(data, 4*i))
			}
			if code >= count {
				if code == 0 {
					continue // a null row of an empty dictionary
				}
				return nil, ErrMalformed
			}
			if !done[code] {
				s, ok := slice(strs, offsets, code)
				if !ok {
					return nil, ErrMalformed
				}
				words[code], done[code] = s, true
			}
			out[i] = words[code]
		}
	default:
		return nil, errors.New("teigeodata: column " + name + " does not hold strings")
	}
	return out, nil
}

// slice is string i of bytes, between offsets i and i+1.
func slice(bytes, offsets []byte, i int) (string, bool) {
	at, end := int(u32(offsets, 4*i)), int(u32(offsets, 4*i+4))
	if at > end || end > len(bytes) {
		return "", false
	}
	return string(bytes[at:end]), true
}

// Column is the column called name with one value per row: a string for a string column, the
// column's own number type (int32, uint8, float64, ...) for a number column, and nil where the
// row is null. It copies; Strings and Values read faster.
func (r *Reader) Column(name string) ([]any, error) {
	c, err := r.column(name)
	if err != nil {
		return nil, err
	}
	out := make([]any, r.rows)
	switch c.ty {
	case TypeUtf8, TypeDict16, TypeDict32:
		err = fill(out, r.Strings, name)
	case TypeI32:
		err = fill(out, valuesOf[int32](r), name)
	case TypeU32:
		err = fill(out, valuesOf[uint32](r), name)
	case TypeI64:
		err = fill(out, valuesOf[int64](r), name)
	case TypeU64:
		err = fill(out, valuesOf[uint64](r), name)
	case TypeF32:
		err = fill(out, valuesOf[float32](r), name)
	case TypeF64:
		err = fill(out, valuesOf[float64](r), name)
	case TypeU8:
		err = fill(out, valuesOf[uint8](r), name)
	case TypeI16:
		err = fill(out, valuesOf[int16](r), name)
	default:
		err = fill(out, valuesOf[uint16](r), name)
	}
	if err != nil {
		return nil, err
	}
	nulls, err := r.Nulls(name)
	if err != nil {
		return nil, err
	}
	for i, null := range nulls {
		if null {
			out[i] = nil
		}
	}
	return out, nil
}

func valuesOf[T Number](r *Reader) func(string) ([]T, error) {
	return func(name string) ([]T, error) { return Values[T](r, name) }
}

func fill[T any](out []any, read func(string) ([]T, error), name string) error {
	values, err := read(name)
	if err != nil {
		return err
	}
	for i, v := range values {
		out[i] = v
	}
	return nil
}

// Record is one row: column name to value, as Column gives it.
type Record map[string]any

// Records is every row as a Record. It copies every value; for many rows, read the columns
// you need with Strings and Values.
func (r *Reader) Records() ([]Record, error) {
	out := make([]Record, r.rows)
	for i := range out {
		out[i] = make(Record, len(r.columns))
	}
	for _, c := range r.columns {
		values, err := r.Column(c.name)
		if err != nil {
			return nil, err
		}
		for i, v := range values {
			out[i][c.name] = v
		}
	}
	return out, nil
}
