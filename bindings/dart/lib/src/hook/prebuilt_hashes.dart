// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it
// uploads; empty in a checkout between releases, where the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{
  'aarch64-apple-darwin': '8676869a5bcbf1031ebe00dcf6bc37e276d9c8dae9757ac49db14262e02967d6',
  'aarch64-apple-ios': 'f8c71b8d0825b6dac883adeede02878b8201657f49699c378b23aefba7d5e1a2',
  'aarch64-apple-ios-sim': '97de8c575ea0f279791755e16f1dd8c03318dd84db42e017db22363a05d085ef',
  'aarch64-linux-android': '28d15346fed29f0cb136d3691dd20473f4ef78ce60e0064b62fc5dd6510cb385',
  'aarch64-pc-windows-msvc': '4148c56a03f8df6bff55909c8e47ed3bb2c18eee8091e7da9ec96be36f488736',
  'aarch64-unknown-linux-gnu': '12670693d2be69bebe351c83f86f1eb670c7f962f748531cfc34cc7c2ab02126',
  'aarch64-unknown-linux-musl': '61369bcbec467fda0ebfea8d11df3b5fe14e4d8d92c7f6d7434358c05c6456a0',
  'armv7-linux-androideabi': 'fd5e3646d39546ac2222a060a13429db95466da283344ec7906b064c90a43021',
  'x86_64-apple-darwin': '4cf7ea84d1d1c38f402a782c07cc08786d9a7395400ed25b4ec6658efa82abb2',
  'x86_64-apple-ios': 'ef001f4d58052e1dd419995c5152ee3fa55cbde9dab3682cd7289fff417f0aab',
  'x86_64-linux-android': '7cd597353c47c0ad9e8d9048276d1797569b1aff45896955f8c292ff24ddacda',
  'x86_64-pc-windows-msvc': '99ac8a4704b24a580e595bd489627ca4f12c7af33b71a839cc54d08d15912034',
  'x86_64-unknown-linux-gnu': 'a8b140074797c28a12b99e5071fcf0b6cb5f98a90dc2977a56ff4f7e0dbc7427',
  'x86_64-unknown-linux-musl': '2e4fe04dae382ff21e722730f2a558990cf537b9761f492e85da850db0648948',
};
