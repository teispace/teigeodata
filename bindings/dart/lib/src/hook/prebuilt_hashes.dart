// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it
// uploads; empty in a checkout between releases, where the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{
  'aarch64-apple-darwin': '4cd3482939845cd341d09b5f64d87744e01d7809e72749d8bcdf15432f779fd9',
  'aarch64-apple-ios': 'fc94ef918cdf2a22884f6276719feff1d40602c82279d19cccd5db7cab4024e3',
  'aarch64-apple-ios-sim': 'c1cb4508b9ac429f7a5adc155800ae7ad4c1feb427d21fcf1ed699479db7a7c4',
  'aarch64-linux-android': 'dc4c8886d0b8beb6e3be393310714f4695a98a2e4252925af341e2278a847161',
  'aarch64-pc-windows-msvc': 'f57141539126af9e01bfb0f6601136ce0b70130f99e77036013d036cd6d9cc6f',
  'aarch64-unknown-linux-gnu': '7b6756e1bbc7236533c54eea15a2abd7b55958cda0633d019c3663eae20fc4ae',
  'aarch64-unknown-linux-musl': '001e6d7a951204dea074f5baf6d79dc3a579f7f1382f0c76baa6771cf72b0e0c',
  'armv7-linux-androideabi': '2f243b656008eb60685ac78fbdaa42d7883520eac4b37a69a692ba2c094a2869',
  'x86_64-apple-darwin': '672597b8e20342a68b712934e43a2cbb97f6f32c59c214195e6cd9ecb2de7642',
  'x86_64-apple-ios': '7208c3d720f3e9396e7ca8cfeb322ee18aaa98f3a7c92088523a7c5fc3d9ed6b',
  'x86_64-linux-android': '924132a64215896dc842c6cddea85cec8cfac925d9a693a01f2ac5ce4220531e',
  'x86_64-pc-windows-msvc': '63b0a3e10d807e276e886cb052a3dcfee0e45a31fb2be1b8f80b3d12aa5adb63',
  'x86_64-unknown-linux-gnu': 'c3fa92540c0803ae7217e675783b66b45f9c2f2c97d979ba0fff87cab89d0f64',
  'x86_64-unknown-linux-musl': 'e6eefb57807d027a789bf1dd8895ef96081491942a5f5589006579b0fee456aa',
};
