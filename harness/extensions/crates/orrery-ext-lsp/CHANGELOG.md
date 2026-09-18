# Changelog

## Unreleased

- Implemented: `hover`, `definition`, `references`, `diagnostics`, over a
  managed LSP client.
- `LspTransport` is injectable, so the suite drives a fake language server
  in-process rather than requiring `rust-analyzer` to be installed.
- One server per language, kept for the life of the bundle: started per call, a
  language server re-indexes the tree on every hover.
- The document is read **through the broker**, so a server is only ever told
  about files the call was allowed to read.
- The manifest no longer declares `symbols`, which was never implemented.
