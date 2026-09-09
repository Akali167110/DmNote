import { useEffect } from 'react';
import { createCustomJsRuntime } from '@src/renderer/plugins/runtime/customJsRuntime';
import { subscribe } from '@api/modules/shared';

export function useCustomJsInjection(enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let runtime = createCustomJsRuntime();
    const unsubscribe = subscribe<{
      pluginAuthorityResetRequired?: boolean;
    }>('web:reconnected', (payload) => {
      if (
        disposed ||
        window.__dmn_runtime !== 'web' ||
        payload?.pluginAuthorityResetRequired !== true
      )
        return;
      runtime.dispose();
      runtime = createCustomJsRuntime();
      runtime.initialize();
    });
    runtime.initialize();

    return () => {
      disposed = true;
      unsubscribe();
      runtime.dispose();
    };
  }, [enabled]);
}
