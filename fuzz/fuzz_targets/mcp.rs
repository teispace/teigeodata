//! libFuzzer on [`teigeodata_server::fuzz::mcp`] (D-331): every MCP tool, with arguments built
//! from its own input schema, and resource reads, on the bindings' synthetic fixture.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_server::fuzz::mcp(data));
