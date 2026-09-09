export interface InvokeRequest {
  requestId: string;
  command: string;
  args: Record<string, unknown>;
}

/** 전송 오류를 포함한 동기 예외는 해당 요청의 거부로 전달 */
export interface IpcTransport {
  sendInvoke: (request: InvokeRequest) => void;
}

/** 문자열뿐 아니라 편집 오류의 code·details·retryable도 원형 유지 */
export type InvokeResponse =
  | { requestId: string; ok: true; result: unknown }
  | { requestId: string; ok: false; error: unknown };

export interface IpcShimConfig {
  transport: IpcTransport;
  convertFileSrc: (filePath: string, protocol?: string) => string;
  metadata: {
    currentWindow: { label: string };
    currentWebview: { windowLabel: string; label: string };
  };
  /** 생략 시 응답 지연만으로 요청 실패를 판정하지 않음 */
  requestTimeout?: {
    milliseconds: number;
    error: (command: string) => unknown;
  };
  /** 생략 시 기존 OBS처럼 emit와 emit_to를 현재 문서에 전달 */
  emit?: (
    command: 'plugin:event|emit' | 'plugin:event|emit_to',
    args: Record<string, unknown>,
  ) => unknown | Promise<unknown>;
}

/** 전송·자산·권한 정책과 독립적인 Tauri JS API 호환 계층 */
export function createIpcShim(config: IpcShimConfig) {
  const callbacks = new Map<number, (data: unknown) => void>();
  const listeners = new Map<number, { event: string; handlerId: number }>();
  const listenersByName = new Map<string, Set<number>>();
  const pending = new Map<
    string,
    {
      resolve: (value: unknown) => void;
      reject: (reason: unknown) => void;
      timer?: ReturnType<typeof setTimeout>;
    }
  >();
  let nextEventId = 1;
  let nextRequestId = 1;
  const requestPrefix = Array.from(
    crypto.getRandomValues(new Uint32Array(4)),
    (value) => value.toString(16),
  ).join('_');
  let disposed = false;

  function registerCallback(callback?: (data: unknown) => void, once = false) {
    let id: number;
    do {
      id = crypto.getRandomValues(new Uint32Array(1))[0];
    } while (callbacks.has(id));
    callbacks.set(id, (data) => {
      if (once) callbacks.delete(id);
      callback?.(data);
    });
    return id;
  }

  function unregisterCallback(id: number) {
    callbacks.delete(id);
  }

  function runCallback(id: number, data: unknown) {
    callbacks.get(id)?.(data);
  }

  function unlisten(event: string, eventId: number) {
    const entry = listeners.get(eventId);
    if (entry) {
      unregisterCallback(entry.handlerId);
      listeners.delete(eventId);
    }
    const ids = listenersByName.get(event);
    ids?.delete(eventId);
    if (ids?.size === 0) listenersByName.delete(event);
  }

  function dispatchEvent(event: string, payload: unknown) {
    const ids = listenersByName.get(event);
    if (!ids) return;
    for (const id of ids) {
      const entry = listeners.get(id);
      if (entry) runCallback(entry.handlerId, { event, id, payload });
    }
  }

  function receiveResponse(response: InvokeResponse) {
    const request = pending.get(response.requestId);
    if (!request) return;
    pending.delete(response.requestId);
    if (request.timer !== undefined) clearTimeout(request.timer);
    if (response.ok === true) request.resolve(response.result);
    else request.reject(response.error);
  }

  async function invoke(
    command: string,
    args: Record<string, unknown> = {},
    _options?: unknown,
  ): Promise<unknown> {
    if (disposed) throw new Error('[IPC Shim] Disposed');
    if (command === 'plugin:event|listen') {
      const event = args.event as string;
      const id = nextEventId++;
      listeners.set(id, { event, handlerId: args.handler as number });
      if (!listenersByName.has(event)) listenersByName.set(event, new Set());
      listenersByName.get(event)!.add(id);
      return id;
    }
    if (command === 'plugin:event|unlisten') {
      unlisten(args.event as string, args.eventId as number);
      return;
    }
    if (command === 'plugin:event|emit' || command === 'plugin:event|emit_to') {
      if (config.emit) return config.emit(command, args);
      dispatchEvent(args.event as string, args.payload);
      return;
    }

    const requestId = `rpc_${requestPrefix}_${nextRequestId++}`;
    return new Promise((resolve, reject) => {
      const request: {
        resolve: (value: unknown) => void;
        reject: (reason: unknown) => void;
        timer?: ReturnType<typeof setTimeout>;
      } = { resolve, reject };
      pending.set(requestId, request);
      if (config.requestTimeout) {
        const timeout = config.requestTimeout;
        request.timer = setTimeout(() => {
          receiveResponse({
            requestId,
            ok: false,
            error: timeout.error(command),
          });
        }, timeout.milliseconds);
      }
      try {
        config.transport.sendInvoke({ requestId, command, args });
      } catch (error) {
        receiveResponse({ requestId, ok: false, error });
      }
    });
  }

  const internals = {
    invoke,
    transformCallback: registerCallback,
    unregisterCallback,
    runCallback,
    callbacks,
    convertFileSrc: config.convertFileSrc,
    metadata: config.metadata,
  };

  /** 프레임마다 별도 shim과 대상 전역을 사용 */
  function installGlobals(target: object, flagTarget: object = target) {
    Object.assign(target, {
      __TAURI_INTERNALS__: internals,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: unlisten },
    });
    Object.assign(flagTarget, { isTauri: true });
  }

  function dispose(reason: unknown = new Error('[IPC Shim] Disposed')) {
    disposed = true;
    for (const requestId of pending.keys()) {
      receiveResponse({ requestId, ok: false, error: reason });
    }
    callbacks.clear();
    listeners.clear();
    listenersByName.clear();
  }

  return { internals, installGlobals, dispatchEvent, receiveResponse, dispose };
}

/**
 * JSON 기반 명령 인자의 Tauri 직렬화 훅 처리.
 * MessagePort 등 자동 JSON 변환이 없는 호스트에서도 Channel 식별자 유지.
 * 바이너리 전송은 별도 호스트 계약 사용.
 */
export function serializeIpcArgs(
  args: Record<string, unknown>,
): Record<string, unknown> {
  return JSON.parse(
    JSON.stringify(args, (_key, value: unknown) => {
      if (value !== null && typeof value === 'object') {
        const serialize = (value as Record<string, unknown>)
          .__TAURI_TO_IPC_KEY__;
        if (typeof serialize === 'function') return serialize.call(value);
      }
      return value;
    }),
  ) as Record<string, unknown>;
}
