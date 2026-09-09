import type {} from '@dmnote/editor/globals';
import {
  createIpcShim,
  serializeIpcArgs,
  type InvokeRequest,
} from '@dmnote/ipc-shim';
import {
  WEB_PROTOCOL_VERSION,
  webHostError,
  type PortLike,
  type WebClientMessage,
  type WebServerMessage,
  type WebRole,
  type WebDownload,
} from '../protocol';
import { AssetUrlRegistry } from '../browser/assetUrls';
import { attachBrowserKeyboard } from '../browser/keyboard';
import { createBrowserClientIdentity } from '../browser/clientIdentity';
import type { WebOverlayState } from '../browser/overlay';
import {
  createWebAudioPreview,
  type WebAudioPreview,
  type WebInputSound,
} from '../browser/audio';
import { pickBrowserFiles, downloadBrowserFile } from '../browser/files';
import type { CssImportFetcher } from '../browser/cssImports';
import { prepareFileCommand, type BrowserFilePicker } from './fileCommands';

export interface SharedWorkerConnection {
  port: PortLike;
  addEventListener?(type: 'error', listener: (event: Event) => void): void;
  removeEventListener?(type: 'error', listener: (event: Event) => void): void;
}
export type WebConnectionState =
  | 'connecting'
  | 'connected'
  | 'disconnected'
  | 'disposed';
export interface ConnectWebEditorOptions {
  workerUrl: string | URL;
  documentId: string;
  role: WebRole;
  clientId?: string;
  workerName?: string;
  connectTimeoutMs?: number;
  keyboard?: boolean;
  audio?: boolean;
  onOverlayState?: (state: WebOverlayState) => void;
  fetchCssImport?: CssImportFetcher;
  pickFiles?: BrowserFilePicker;
  download?: (file: WebDownload) => void | Promise<void>;
  /** 별도 브라우저 환경·통합 테스트의 실제 MessagePort 연결 주입 */
  createWorker?: (url: string | URL, name: string) => SharedWorkerConnection;
}
export interface WebEditorClient {
  readonly clientId: string;
  readonly documentId: string;
  readonly role: WebRole;
  readonly assets: AssetUrlRegistry;
  readonly connectionState: WebConnectionState;
  invoke<T = unknown>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T>;
  reconnect(): Promise<void>;
  dispose(): void;
  subscribeConnectionState(
    listener: (state: WebConnectionState, error?: unknown) => void,
  ): () => void;
  installEditorApi(): Promise<unknown>;
}

type RequestMessage = Extract<WebClientMessage, { type: 'invoke' | 'emit' }>;
type ResponseMessage = Extract<WebServerMessage, { type: 'response' }>;
interface PendingRequest {
  rawRestore: boolean;
  message: RequestMessage;
  resolve: (result: unknown) => void;
  reject: (error: unknown) => void;
}
interface PreviewSubscription {
  args: Record<string, unknown>;
  callbackId: number;
  nextIndex: number;
  offset: number;
}

const unsupportedNativeCommands = new Set([
  'app_auto_update',
  'app_restart',
  'app_quit',
  'window_close',
  'window_minimize',
  'window_open_devtools_all',
  'window_show_main',
  'obs_start',
  'obs_stop',
  'obs_status',
  'obs_regenerate_token',
  'key_sound_set_latency_logging',
  'key_sound_get_output_state',
  'key_sound_list_output_devices',
  'key_sound_set_output_backend',
]);

/** SharedWorker의 세션 준비 후에만 현재 문서의 Tauri 호환 API 설치 */
export async function connectWebEditor(
  options: ConnectWebEditorOptions,
): Promise<WebEditorClient> {
  if (!options.documentId.trim()) throw new Error('documentId is required');
  const identity = options.clientId
    ? null
    : await createBrowserClientIdentity(
        options.documentId,
        options.role,
        options.workerName ?? 'dmnote-web-editor',
      );
  const clientId = options.clientId ?? identity!.clientId;
  const assets = new AssetUrlRegistry();
  const pending = new Map<string, PendingRequest>();
  const previews = new Map<number, PreviewSubscription>();
  const stateListeners = new Set<
    (state: WebConnectionState, error?: unknown) => void
  >();
  const abort = new AbortController();
  let state: WebConnectionState = 'connecting';
  let sequence = 0;
  let rawSubscriptions = 0;
  let audio: WebAudioPreview | null = null;
  let connection: SharedWorkerConnection | null = null;
  let detachConnection: (() => void) | null = null;
  let cancelConnectionAttempt: ((error: unknown) => void) | null = null;
  let connecting: Promise<void> | null = null;
  let hasConnected = false;
  let keyboard: ReturnType<typeof attachBrowserKeyboard> | null = null;
  let disposed = false;
  let processing = Promise.resolve();
  const requestPrefix = crypto.randomUUID();
  const realmId = crypto.randomUUID();
  const requestId = () => `web_${clientId}_${requestPrefix}_${++sequence}`;
  const transition = (next: WebConnectionState, error?: unknown) => {
    state = next;
    for (const listener of stateListeners) {
      try {
        listener(next, error);
      } catch (listenerError) {
        console.error('Web connection listener failed', listenerError);
      }
    }
  };
  const disconnected = (error: unknown) => {
    if (disposed) return;
    transition('disconnected', error);
    keyboard?.releaseAll();
  };
  const post = (message: WebClientMessage): void => {
    if (!connection || state !== 'connected') return;
    try {
      connection.port.postMessage(message);
    } catch (error) {
      if (
        error instanceof Error &&
        error.name === 'DataCloneError' &&
        'requestId' in message
      ) {
        const request = pending.get(message.requestId);
        pending.delete(message.requestId);
        request?.reject(error);
      } else disconnected(error);
    }
  };
  const request = (
    message: RequestMessage,
    rawRestore = false,
  ): Promise<unknown> =>
    new Promise((resolve, reject) => {
      if (disposed) {
        reject(webHostError('HOST_DISPOSED', 'Web editor connection disposed'));
        return;
      }
      pending.set(message.requestId, { message, resolve, reject, rawRestore });
      post(message);
    });
  const receiveResponse = async (message: ResponseMessage) => {
    const waiting = pending.get(message.requestId);
    if (!waiting) return;
    pending.delete(message.requestId);
    if (message.ok === false) {
      waiting.reject(message.error);
      return;
    }
    try {
      if (message.assets) {
        assets.hydrate(message.assets);
        audio?.invalidate(message.assets.map((asset) => asset.key));
      }
      if (message.download)
        await (options.download ?? downloadBrowserFile)(message.download);
      waiting.resolve(message.result);
    } catch (error) {
      waiting.reject(error);
    }
  };
  const invoke = async (
    command: string,
    args: Record<string, unknown> = {},
    id = requestId(),
  ): Promise<unknown> => {
    if (disposed)
      throw webHostError('HOST_DISPOSED', 'Web editor connection disposed');
    if (
      unsupportedNativeCommands.has(command) ||
      command.startsWith('panel_window_') ||
      command.startsWith('panel_drag_')
    ) {
      throw webHostError(
        'UNSUPPORTED_WEB_COMMAND',
        `Native command is not available in the browser: ${command}`,
      );
    }
    if (command === 'get_cursor_settings')
      return {
        size: 1,
        base_size: 24,
        fill_color: '#000000',
        outline_color: '#FFFFFF',
        is_macos: /Mac/.test(navigator.platform),
      };
    if (command === 'css_fetch_import') {
      if (!options.fetchCssImport)
        throw webHostError(
          'CSS_INTERMEDIARY_NOT_CONFIGURED',
          'A CSS intermediary fetcher is required',
        );
      if (typeof args.url !== 'string')
        throw new TypeError('CSS URL must be a string');
      return options.fetchCssImport(args.url);
    }
    if (command === 'app_open_external') {
      if (typeof args.url !== 'string')
        throw new TypeError('External URL must be a string');
      const url = new URL(args.url);
      if (!['http:', 'https:'].includes(url.protocol))
        throw webHostError(
          'UNSUPPORTED_EXTERNAL_PROTOCOL',
          `Cannot open ${url.protocol} in this host`,
        );
      const opened = window.open(url.href, '_blank');
      if (!opened)
        throw webHostError(
          'POPUP_BLOCKED',
          'The browser blocked the external window',
        );
      opened.opener = null;
      return;
    }
    const serializable = serializeIpcArgs(args);
    const prepared = await prepareFileCommand(
      command,
      serializable,
      options.pickFiles ?? pickBrowserFiles,
      abort.signal,
    );
    if (prepared.cancelled) return prepared.result;
    let newPreview: number | undefined;
    if (command === 'editor_preview_subscribe') {
      const channel = prepared.args.channel;
      if (typeof channel !== 'string' || !/^__CHANNEL__:\d+$/.test(channel))
        throw new TypeError('Invalid preview Channel');
      const callbackId = Number(channel.slice('__CHANNEL__:'.length));
      if (!previews.has(callbackId)) {
        newPreview = callbackId;
        previews.set(callbackId, {
          callbackId,
          args: prepared.args,
          nextIndex: 0,
          offset: 0,
        });
      }
    }
    const result = await request({
      type: 'invoke',
      requestId: id,
      command,
      args: prepared.args,
      ...(prepared.files ? { files: prepared.files } : {}),
    }).catch((error) => {
      if (newPreview !== undefined) previews.delete(newPreview);
      throw error;
    });
    if (command === 'raw_input_subscribe') rawSubscriptions += 1;
    else if (command === 'raw_input_unsubscribe')
      rawSubscriptions = Math.max(0, rawSubscriptions - 1);
    return result;
  };
  const shim = createIpcShim({
    transport: {
      sendInvoke: (call: InvokeRequest) => {
        void invoke(call.command, call.args, call.requestId).then(
          (result) =>
            shim.receiveResponse({
              requestId: call.requestId,
              ok: true,
              result,
            }),
          (error) =>
            shim.receiveResponse({
              requestId: call.requestId,
              ok: false,
              error,
            }),
        );
      },
    },
    convertFileSrc: (key) => assets.resolve(key),
    metadata: {
      currentWindow: { label: options.role },
      currentWebview: { label: options.role, windowLabel: options.role },
    },
    emit: (command, args) =>
      request({
        type: 'emit',
        requestId: requestId(),
        event: String(args.event),
        payload: args.payload,
        ...(command === 'plugin:event|emit_to' ? { target: args.target } : {}),
      }),
  });
  const receive = async (message: WebServerMessage) => {
    if (message.type === 'response') await receiveResponse(message);
    else if (message.type === 'event') {
      if (message.event === 'web:overlay-state' && options.onOverlayState) {
        try {
          options.onOverlayState(message.payload as WebOverlayState);
        } catch (error) {
          shim.dispatchEvent(
            'web:error',
            webHostError(
              'OVERLAY_PRESENTATION_FAILED',
              error instanceof Error ? error.message : String(error),
            ),
          );
        }
      }
      if (message.event === 'web:input-sound' && audio)
        void audio
          .play(message.payload as WebInputSound)
          .catch((error) =>
            shim.dispatchEvent(
              'web:error',
              webHostError(
                'AUDIO_PLAYBACK_FAILED',
                error instanceof Error ? error.message : String(error),
              ),
            ),
          );
      shim.dispatchEvent(message.event, message.payload);
    } else if (message.type === 'assets') {
      assets.hydrate(message.assets);
      audio?.invalidate(message.assets.map((asset) => asset.key));
      if (message.removed) {
        assets.remove(message.removed);
        audio?.invalidate(message.removed);
      }
    } else if (message.type === 'callback') {
      const subscription = previews.get(message.callbackId);
      let payload = message.payload;
      if (
        subscription &&
        payload &&
        typeof payload === 'object' &&
        'index' in payload &&
        typeof payload.index === 'number'
      ) {
        const index = payload.index + subscription.offset;
        if (!('end' in payload))
          subscription.nextIndex = Math.max(subscription.nextIndex, index + 1);
        const ended = 'end' in payload;
        payload = { ...payload, index };
        if (ended) previews.delete(message.callbackId);
      }
      shim.internals.runCallback(message.callbackId, payload);
    } else if (message.type === 'error') disconnected(message.error);
  };
  const createConnection = (): Promise<void> => {
    if (disposed)
      return Promise.reject(
        webHostError('HOST_DISPOSED', 'Web editor connection disposed'),
      );
    keyboard?.releaseAll();
    transition('connecting');
    detachConnection?.();
    connection?.port.close();
    const factory =
      options.createWorker ??
      ((url: string | URL, name: string) => {
        if (typeof SharedWorker === 'undefined')
          throw webHostError(
            'SHARED_WORKER_UNAVAILABLE',
            'This browser does not support SharedWorker',
          );
        return new SharedWorker(url, { type: 'module', name });
      });
    let next: SharedWorkerConnection;
    try {
      next = factory(
        options.workerUrl,
        options.workerName ?? 'dmnote-web-editor',
      );
    } catch (error) {
      disconnected(error);
      throw error;
    }
    connection = next;
    const connectRequestId = requestId();
    return new Promise<void>((resolve, reject) => {
      let ready = false;
      let finished = false;
      const fail = (error: unknown) => {
        if (finished || connection !== next) return;
        finished = true;
        clearTimeout(timeout);
        if (!ready) reject(error);
        disconnected(error);
        detachConnection?.();
        next.port.close();
        if (connection === next) connection = null;
      };
      cancelConnectionAttempt = fail;
      const timeout = setTimeout(() => {
        if (!ready)
          fail(
            webHostError(
              'CONNECTION_TIMEOUT',
              'SharedWorker did not finish session initialization',
              true,
            ),
          );
      }, options.connectTimeoutMs ?? 15000);
      const workerError = () =>
        fail(
          webHostError(
            'WORKER_DISCONNECTED',
            'SharedWorker execution failed',
            true,
          ),
        );
      const messageError = () =>
        fail(
          webHostError(
            'MESSAGE_DECODE_FAILED',
            'Cannot decode SharedWorker message',
            true,
          ),
        );
      const message = (event: MessageEvent) => {
        const value = event.data as WebServerMessage;
        if (!value || typeof value !== 'object') {
          messageError();
          return;
        }
        processing = processing
          .then(async () => {
            if (connection !== next || disposed || finished) return;
            if (
              value.type === 'ready' &&
              value.requestId === connectRequestId
            ) {
              if (
                value.clientId !== clientId ||
                value.documentId !== options.documentId ||
                value.role !== options.role
              )
                throw webHostError(
                  'SESSION_IDENTITY_MISMATCH',
                  'SharedWorker attached a different session',
                );
              assets.hydrate(value.assets);
              audio?.invalidate();
              clearTimeout(timeout);
              ready = true;
              cancelConnectionAttempt = null;
              transition('connected');
              const reconnecting = hasConnected;
              hasConnected = true;
              for (const subscription of previews.values())
                subscription.offset = subscription.nextIndex;
              // worker 구독 계수는 연결마다 0에서 재구성. 응답 대기 unsubscribe보다 먼저 복원
              for (const [id, waiting] of pending)
                if (waiting.rawRestore) {
                  pending.delete(id);
                  waiting.resolve(null);
                }
              const replay = [...pending.values()];
              for (let i = 0; i < rawSubscriptions; i += 1)
                void request(
                  {
                    type: 'invoke',
                    requestId: requestId(),
                    command: 'raw_input_subscribe',
                    args: {},
                  },
                  true,
                ).catch((error) => disconnected(error));
              for (const waiting of replay) post(waiting.message);
              if (reconnecting) {
                for (const subscription of previews.values()) {
                  const replayedByPending = [...pending.values()].some(
                    ({ message: waiting }) =>
                      waiting.type === 'invoke' &&
                      waiting.command === 'editor_preview_subscribe' &&
                      waiting.args.channel === subscription.args.channel,
                  );
                  if (!replayedByPending)
                    void request({
                      type: 'invoke',
                      requestId: requestId(),
                      command: 'editor_preview_subscribe',
                      args: subscription.args,
                    }).catch((error) => transition('disconnected', error));
                }
                shim.dispatchEvent('obs:resync', null);
                shim.dispatchEvent('web:reconnected', {
                  sessionId: value.sessionId,
                  generation: value.generation,
                  pluginAuthorityResetRequired:
                    value.pluginAuthorityResetRequired === true,
                });
              }
              resolve();
            } else if (value.type === 'error' && !ready) fail(value.error);
            else if (ready) await receive(value);
          })
          .catch(fail);
      };
      next.port.addEventListener('message', message);
      next.port.addEventListener('messageerror', messageError);
      next.addEventListener?.('error', workerError);
      detachConnection = () => {
        clearTimeout(timeout);
        next.port.removeEventListener('message', message);
        next.port.removeEventListener('messageerror', messageError);
        next.removeEventListener?.('error', workerError);
      };
      try {
        next.port.start();
        next.port.postMessage({
          type: 'connect',
          version: WEB_PROTOCOL_VERSION,
          requestId: connectRequestId,
          documentId: options.documentId,
          role: options.role,
          clientId,
          realmId,
        } satisfies WebClientMessage);
      } catch (error) {
        fail(error);
      }
    });
  };
  const reconnect = (): Promise<void> => {
    if (!connecting)
      connecting = Promise.resolve()
        .then(async () => {
          await identity?.reacquire();
          await createConnection();
        })
        .finally(() => {
          connecting = null;
        });
    return connecting;
  };
  const pagehide = () => {
    if (disposed) return;
    keyboard?.releaseAll();
    post({ type: 'disconnect' });
    cancelConnectionAttempt?.(
      webHostError('PAGE_SUSPENDED', 'Browser page suspended', true),
    );
    detachConnection?.();
    connection?.port.close();
    connection = null;
    identity?.release();
    transition('disconnected');
  };
  const pageshow = (event: PageTransitionEvent) => {
    if (event.persisted && !disposed)
      void reconnect().catch((error) => disconnected(error));
  };
  const dispose = () => {
    if (disposed) return;
    keyboard?.dispose();
    post({ type: 'disconnect' });
    disposed = true;
    const error = webHostError(
      'HOST_DISPOSED',
      'Web editor connection disposed',
    );
    cancelConnectionAttempt?.(error);
    cancelConnectionAttempt = null;
    abort.abort();
    window.removeEventListener('pagehide', pagehide);
    window.removeEventListener('pageshow', pageshow);
    identity?.release();
    detachConnection?.();
    connection?.port.close();
    connection = null;
    for (const waiting of pending.values()) waiting.reject(error);
    pending.clear();
    previews.clear();
    shim.dispose(error);
    audio?.dispose();
    assets.dispose();
    transition('disposed');
    stateListeners.clear();
  };
  try {
    await reconnect();
  } catch (error) {
    dispose();
    throw error;
  }
  window.addEventListener('pagehide', pagehide);
  window.addEventListener('pageshow', pageshow);
  shim.installGlobals(window, globalThis);
  Object.assign(window, {
    __dmn_window_type: options.role,
    __dmn_runtime: 'web',
  });
  if (options.audio !== false)
    audio = createWebAudioPreview({
      resolveAsset: (key) => assets.resolve(key),
      onError: (error) =>
        shim.dispatchEvent(
          'web:error',
          webHostError(
            'AUDIO_PLAYBACK_FAILED',
            error instanceof Error ? error.message : String(error),
          ),
        ),
    });
  if (options.keyboard !== false) keyboard = attachBrowserKeyboard(post);
  return {
    clientId,
    documentId: options.documentId,
    role: options.role,
    assets,
    get connectionState() {
      return state;
    },
    invoke: <T = unknown>(command: string, args?: Record<string, unknown>) =>
      shim.internals.invoke(command, args) as Promise<T>,
    reconnect,
    dispose,
    subscribeConnectionState(listener) {
      stateListeners.add(listener);
      return () => stateListeners.delete(listener);
    },
    installEditorApi: () => import('@dmnote/editor/install'),
  };
}
