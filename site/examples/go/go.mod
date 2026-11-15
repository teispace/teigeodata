module github.com/teispace/teigeodata/site/examples/go

go 1.26

require github.com/teispace/teigeodata/bindings/go v0.0.0

require github.com/ebitengine/purego v0.11.3 // indirect

// The tour builds against this checkout; an application leaves this line out.
replace github.com/teispace/teigeodata/bindings/go => ../../../bindings/go
