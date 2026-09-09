import { createIpcShim } from '@dmnote/ipc-shim';

const unexpectedCommands: string[] = [];
const shim = createIpcShim({
  transport: {
    sendInvoke(request) {
      unexpectedCommands.push(request.command);
      throw new Error(`Unimplemented fixture host command: ${request.command}`);
    },
  },
  convertFileSrc: (file) => file,
  metadata: {
    currentWindow: { label: 'overlay' },
    currentWebview: { label: 'overlay', windowLabel: 'overlay' },
  },
});
shim.installGlobals(window, globalThis);
Object.assign(window, {
  __dmn_window_type: 'overlay',
  __EDITOR_PACKAGE_HOST__: { shim, unexpectedCommands },
});
