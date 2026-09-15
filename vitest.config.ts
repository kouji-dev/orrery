import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['src/test-setup.ts'],
    include: ['src/**/*.spec.ts', 'scripts/**/*.spec.ts', 'tools/**/*.spec.ts'],
    // Angular component specs render real templates in jsdom; under a full
    // parallel run the heavy modals exceed vitest's 5 s default on this box.
    testTimeout: 15_000,
  },
});
