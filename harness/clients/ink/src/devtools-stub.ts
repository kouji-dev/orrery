/**
 * Ink imports `react-devtools-core` unconditionally and only *uses* it when
 * `DEV=true`. Bundling the real thing would put a browser devtools client in a
 * terminal binary, so the bundle aliases it to this.
 */

export default {
  connectToDevTools(): void {
    // Nothing: a bundled client has no devtools to connect to.
  },
};
