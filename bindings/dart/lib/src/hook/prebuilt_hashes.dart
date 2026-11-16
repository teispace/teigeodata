// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it
// uploads; empty in a checkout between releases, where the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{
  'aarch64-apple-darwin': 'acb2124b728cf53796195d7b7b9bf5e6a91667569ed7b35d3ae176d5460f353b',
  'aarch64-apple-ios': 'c83c3ca3bdd08da54209c63b66d816a40b2ad5743b99c9ffc7a42ab3ccf3a85a',
  'aarch64-apple-ios-sim': '173cae1a24ec7737210147fde21c3b4ea01d785bbf32ae7176841392a0aed796',
  'aarch64-linux-android': 'd62f70a80b440868d8f722102a76ddd6441827f1fac094dce3402f36e3f5980d',
  'aarch64-pc-windows-msvc': 'f88a610a9a642016b34299999564e8bf67a3167e04284c1f33a082818959a02a',
  'aarch64-unknown-linux-gnu': '5a3270e5d4dc3ee4cbf7cf3a37741e52528d2c1d52cb58877a1019fcd4e18e88',
  'aarch64-unknown-linux-musl': '4af89dfcfa79c6bc7fb480c637d40ea3921c5d12c51e5c55a7c17b9299df545f',
  'armv7-linux-androideabi': 'c5f3c1c044e812bde63c1574695fc0ee07e23d7a89945defad59fc2a5f8b50ba',
  'x86_64-apple-darwin': 'cdf54918a26df283470e9c95e16a58fd90858a543629ff2d33a62b7d0c2fdeef',
  'x86_64-apple-ios': 'c4226bc007a0aaa143c6cf057d9bd9f4f51f2bb8303ed551fe08a43ee0ca996e',
  'x86_64-linux-android': '06bbed8a0bed0d5f5c52167828b61c9a56a32ccc464c950f10a58b8ae26c93b4',
  'x86_64-pc-windows-msvc': '9a02e498c38390ec1a421cbb42bd2cd465b5f5cc5ce69d9ddcfbf63893d22e97',
  'x86_64-unknown-linux-gnu': '7faa776a97f41e5b2c314fceb3f46a1c61fe49a72b0e85bc397530424cad782b',
  'x86_64-unknown-linux-musl': '052c7c02fb0bb8c76c8e3ccdf5de77b4be474642f11f2f1120ba081a1f9d9839',
};
