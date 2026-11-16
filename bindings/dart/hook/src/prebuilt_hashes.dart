// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it
// uploads; empty in a checkout between releases, where the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{
  'aarch64-apple-darwin': 'e6aa531f7d8941a9d217d5ec8363ec1343947f6006143ae205a001bf9b910af6',
  'aarch64-apple-ios': '6619989f5da08b5d5fd2636c0a4b381c1b044e2a3ddc965483d94b03c13c162e',
  'aarch64-apple-ios-sim': '30579803f5e392fb654d064137cb1eeb93d3e9c6685a69984c20e1e697c75883',
  'aarch64-linux-android': '3de78bdb3524d397c5c37c327becafd80bd773c631b2a9f108cca2f33439eb2b',
  'aarch64-pc-windows-msvc': '7a6f881fc712bc7cd7a84aa60027dd7ccab8c148937e77b5278f74f56fd7d709',
  'aarch64-unknown-linux-gnu': 'dc729dc9b0eadf1790133148dedc91f1e2abcfb7c8b291f4cfa1aba6f6daf02d',
  'aarch64-unknown-linux-musl': 'baf9166a0f019141dd240284b31c158057b1b3c868a472991d3db2fd66b3ef9c',
  'armv7-linux-androideabi': '38a6f767367c1c27f19cd6192fb4aa79bf45991e3bd2073f9f60d3edbe4767b8',
  'x86_64-apple-darwin': '8af815b15818adb04d7456376c1b44fd5a2ac28fd268f24f0033cc44026ea8a9',
  'x86_64-apple-ios': '91e95fae5e753dbc12cc61c1552a93d9c2e6c8938f0558a2b90629cdd93d84da',
  'x86_64-linux-android': '190af92c1d7fcad17ec065a9182e98acc2063da56a35fe68517ee274ab8d670e',
  'x86_64-pc-windows-msvc': 'b9eb65f1661d38bb99c6cca1e15ddc95a67c47f3cca5d9b391e286221d2314e5',
  'x86_64-unknown-linux-gnu': 'df9fdd88384fe33c19a0bfc1b7984794c2e5c9a36d3517f49263ccb26a72f95f',
  'x86_64-unknown-linux-musl': '5ae30bb65a62468082fe4b404728842c0ea96ee5cfe94005c6a51372177e65b5',
};
