import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useCustomJsInjection } from './useCustomJsInjection';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const mocks = vi.hoisted(() => ({
  createRuntime: vi.fn(),
  subscribe: vi.fn(),
}));

vi.mock('@src/renderer/plugins/runtime/customJsRuntime', () => ({
  createCustomJsRuntime: mocks.createRuntime,
}));
vi.mock('@api/modules/shared', () => ({ subscribe: mocks.subscribe }));

const Harness = ({ enabled = true }: { enabled?: boolean }) => {
  useCustomJsInjection(enabled);
  return null;
};

describe('useCustomJsInjection 웹 권한 복구', () => {
  const originalRuntime = window.__dmn_runtime;
  let root: Root;
  let container: HTMLDivElement;
  let reconnect: (payload: { pluginAuthorityResetRequired?: boolean }) => void;
  let unsubscribe: ReturnType<typeof vi.fn>;
  let runtimes: {
    initialize: ReturnType<typeof vi.fn>;
    dispose: ReturnType<typeof vi.fn>;
  }[];

  beforeEach(() => {
    vi.clearAllMocks();
    window.__dmn_runtime = 'web';
    runtimes = [];
    unsubscribe = vi.fn();
    mocks.subscribe.mockImplementation((event, listener) => {
      expect(event).toBe('web:reconnected');
      reconnect = listener;
      return unsubscribe;
    });
    mocks.createRuntime.mockImplementation(() => {
      const runtime = { initialize: vi.fn(), dispose: vi.fn() };
      runtimes.push(runtime);
      return runtime;
    });
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    window.__dmn_runtime = originalRuntime;
  });

  it('기본 초기화와 정리는 런타임 하나를 사용한다', () => {
    act(() => root.render(<Harness />));
    expect(runtimes).toHaveLength(1);
    expect(runtimes[0].initialize).toHaveBeenCalledOnce();
    act(() => root.render(<Harness enabled={false} />));
    expect(runtimes[0].dispose).toHaveBeenCalledOnce();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });

  it('실행 권한이 유지되는 재연결은 플러그인을 다시 주입하지 않는다', () => {
    act(() => root.render(<Harness />));
    reconnect({ pluginAuthorityResetRequired: false });
    reconnect({});
    expect(runtimes).toHaveLength(1);
    expect(runtimes[0].dispose).not.toHaveBeenCalled();
  });

  it('권한 재설정이 필요한 재연결은 이전 런타임 정리 후 새로 초기화한다', () => {
    act(() => root.render(<Harness />));
    reconnect({ pluginAuthorityResetRequired: true });
    expect(runtimes).toHaveLength(2);
    expect(runtimes[0].dispose).toHaveBeenCalledOnce();
    expect(runtimes[1].initialize).toHaveBeenCalledOnce();
    expect(runtimes[0].dispose.mock.invocationCallOrder[0]).toBeLessThan(
      runtimes[1].initialize.mock.invocationCallOrder[0],
    );
    act(() => root.render(<Harness enabled={false} />));
    expect(runtimes[1].dispose).toHaveBeenCalledOnce();
    reconnect({ pluginAuthorityResetRequired: true });
    expect(runtimes).toHaveLength(2);
  });

  it('네이티브에서 같은 이름의 이벤트를 받아도 재주입하지 않는다', () => {
    window.__dmn_runtime = 'tauri';
    act(() => root.render(<Harness />));
    reconnect({ pluginAuthorityResetRequired: true });
    expect(runtimes).toHaveLength(1);
    expect(runtimes[0].dispose).not.toHaveBeenCalled();
  });

  it('비활성화 상태에서는 연결 이벤트를 구독하거나 런타임을 만들지 않는다', () => {
    act(() => root.render(<Harness enabled={false} />));
    expect(mocks.createRuntime).not.toHaveBeenCalled();
    expect(mocks.subscribe).not.toHaveBeenCalled();
  });
});
