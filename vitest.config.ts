import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          environment: 'node',
          include: ['web/**/*.test.ts'],
          exclude: ['web/**/*.browser.test.ts'],
        },
      },
      {
        test: {
          name: 'browser',
          include: ['web/**/*.browser.test.ts'],
          // The sandbox-compatible Chromium configuration is one process, so
          // browser files must not race separate pages inside that process.
          fileParallelism: false,
          maxWorkers: 1,
          browser: {
            enabled: true,
            headless: true,
            // The desktop workspace sandbox denies Chromium's macOS Mach-port
            // rendezvous for child renderers. Browser tests still execute in a
            // real local Chromium process; single-process mode keeps that
            // process inside the permitted boundary and is deterministic in CI.
            provider: playwright({
              launchOptions: { args: ['--single-process'] },
            }),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
})
