import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import svgr from 'vite-plugin-svgr';
import path from 'node:path';
import tsconfig from './tsconfig.json';

const aliases = Object.fromEntries(
  Object.entries(tsconfig.compilerOptions.paths)
    .filter(([key]) => key.endsWith('/*'))
    .map(([key, targets]) => [
      key.slice(0, -2),
      path.resolve(__dirname, targets[0].slice(0, -2)),
    ]),
);

export default defineConfig({
  resolve: { alias: aliases },
  plugins: [
    react({
      babel: {
        plugins: [
          [
            'babel-plugin-react-compiler',
            {
              sources: (filename: string) =>
                !filename.includes('stores/signals/'),
            },
          ],
        ],
      },
    }),
    svgr({ include: '**/*.svg', svgrOptions: { exportType: 'default' } }),
  ],
  build: {
    outDir: 'packages/editor/dist',
    emptyOutDir: true,
    lib: {
      entry: Object.fromEntries(
        [
          'editor',
          'overlay',
          'runtime',
          'install',
          'model',
          'plugins',
          'style',
          'grid',
          'toolbar',
          'panel-host',
          'dialogs',
        ].map((name) => [
          name,
          path.resolve(__dirname, `packages/editor/src/${name}.ts`),
        ]),
      ),
      formats: ['es'],
      fileName: (_format, name) => `${name}.js`,
      cssFileName: 'editor',
    },
    rollupOptions: {
      external: (id) => /^(react|react-dom)(\/|$)/.test(id),
      onwarn(warning, warn) {
        if (
          warning.code === 'MODULE_LEVEL_DIRECTIVE' &&
          warning.message.includes('use no memo')
        )
          return;
        warn(warning);
      },
    },
  },
});
