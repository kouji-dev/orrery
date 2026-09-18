import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.tsx"],
    environment: "node",
    // Ink drives a real (fake) terminal and the smoke test drives a loopback
    // HTTP server; both are timers, neither is a model call.
    testTimeout: 20_000,
  },
});
