import { defineConfig } from 'vitest/config';
import path from 'path';

const rendererRoot = path.resolve(__dirname, 'packages/editor/src/renderer');

export default defineConfig({
  resolve: {
    alias: {
      '@dmnote/editor': path.resolve(__dirname, 'packages/editor/src'),
      '@dmnote/ipc-shim': path.resolve(
        __dirname,
        'packages/ipc-shim/src/index.ts',
      ),
      '@components': path.resolve(rendererRoot, 'components'),
      '@styles': path.resolve(rendererRoot, 'styles'),
      '@windows': path.resolve(__dirname, 'src/renderer/windows'),
      '@hooks': path.resolve(rendererRoot, 'hooks'),
      '@api': path.resolve(rendererRoot, 'api'),
      '@assets': path.resolve(rendererRoot, 'assets'),
      '@utils': path.resolve(rendererRoot, 'utils'),
      '@stores': path.resolve(rendererRoot, 'stores'),
      '@constants': path.resolve(rendererRoot, 'constants'),
      '@contexts': path.resolve(rendererRoot, 'contexts'),
      '@plugins': path.resolve(rendererRoot, 'plugins'),
      '@config': path.resolve(rendererRoot, 'config'),
      '@shared': path.resolve(__dirname, 'packages/editor/src/types'),
      '@src': path.resolve(__dirname, 'packages/editor/src/'),
      '@app': path.resolve(__dirname, 'src/'),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./packages/editor/src/renderer/__tests__/setup.ts'],
    include: [
      'src/**/*.test.{ts,tsx}',
      'packages/editor/src/**/*.test.{ts,tsx}',
      'tests/**/*.test.ts',
    ],
  },
});
