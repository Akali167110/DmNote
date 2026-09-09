import type { WebAssetMap, WebEvent } from '../protocol';
import type { PreviewValidation } from '../worker/previewValidation';

export interface EngineSnapshot {
  store: Record<string, unknown>;
  history: Record<string, unknown>;
  pluginModelRevision: number;
  authorityGeneration: number;
  checkpoint: Record<string, unknown>;
}
export interface PreparedCommand {
  token: string;
  store: Record<string, unknown> | null;
  result: unknown;
  events: WebEvent[];
  assetWrites?: WebAssetMap;
  assetDeletes?: string[];
  checkpoint: Record<string, unknown>;
}
export interface EditorEngine {
  readonly recovered?: boolean;
  snapshot(): EngineSnapshot;
  read(command: string, args: Record<string, unknown>): unknown;
  prepare(
    command: string,
    args: Record<string, unknown>,
    token: string,
  ): PreparedCommand;
  confirm(token: string): PreparedCommand;
  discard(token: string): void;
  dispose(): void;
}
export interface KeyMatch {
  mode: string;
  pressedLabel: string | null;
  events: {
    canonical: string;
    slotIndices: number[];
    transition: boolean | null;
    press: boolean;
    canUsePhysicalHoldDuration: boolean;
  }[];
}
export interface KeyboardMatcher {
  update(mappings: unknown, mode: string): void;
  feed(
    physicalId: string,
    candidates: string[],
    pressed: boolean,
    device?: 'keyboard' | 'mouse',
  ): KeyMatch | null;
  active(): { mode: string; keys: string[] };
  clear(): void;
  dispose(): void;
}
export interface EngineFactory {
  preview: PreviewValidation;
  create(
    store?: Record<string, unknown>,
    checkpoint?: Record<string, unknown>,
  ): EditorEngine;
  kind(command: string): 'read' | 'write' | 'unsupported';
  keyboard(mappings: unknown, mode: string): KeyboardMatcher;
}
interface WasmSession {
  snapshot(): string;
  restore_checkpoint(checkpoint: string): void;
  read(command: string, args: string): string;
  prepare_command(command: string, args: string, token: string): string;
  confirm_command(token: string): string;
  discard_command(token: string): void;
  free(): void;
}
interface WasmKeyboard {
  update(mappings: string, mode: string): boolean;
  feed(input: string): string;
  active(): string;
  clear(): void;
  free(): void;
}
interface WasmModule {
  default(): Promise<unknown>;
  WebEditorSession: new (store: string) => WasmSession;
  WebKeyboardMatcher: new (mappings: string, mode: string) => WasmKeyboard;
  web_command_kind(command: string): 'read' | 'write' | 'unsupported';
  validate_preview_json(request: string, label: string): string;
  is_preview_session_id(value: string): boolean;
  migrate_store_json(content: string, timestamp: bigint): string;
}
export const decodeEngineError = (error: unknown): unknown => {
  if (typeof error !== 'string') return error;
  try {
    return JSON.parse(error);
  } catch {
    return error;
  }
};
const parse = <T>(operation: () => string): T => {
  try {
    return JSON.parse(operation()) as T;
  } catch (error) {
    throw decodeEngineError(error);
  }
};
/** wasm-bindgen web 출력을 worker와 같은 패키지에서 로드 */
export const loadWasmEngine = async (
  moduleUrl = new URL(
    /* @vite-ignore */ './wasm/dmnote_editor_engine.js',
    import.meta.url,
  ),
): Promise<EngineFactory> => {
  const wasm = (await import(/* @vite-ignore */ moduleUrl.href)) as WasmModule;
  await wasm.default();
  return {
    preview: {
      validatePublish: (request, label) =>
        parse(() => wasm.validate_preview_json(JSON.stringify(request), label)),
      isSessionId: (value) => wasm.is_preview_session_id(value),
    },
    create(store, checkpoint) {
      const migrated = JSON.parse(
        wasm.migrate_store_json(
          JSON.stringify(store ?? {}),
          BigInt(Date.now()),
        ),
      );
      const initial = migrated.store;
      const session = new wasm.WebEditorSession(JSON.stringify(initial));
      let recovered = Boolean(migrated.repaired);
      if (checkpoint) {
        try {
          session.restore_checkpoint(
            JSON.stringify(
              recovered
                ? {
                    ...checkpoint,
                    editorRevision: initial.editorRevision,
                    authorityAvailable: false,
                  }
                : checkpoint,
            ),
          );
        } catch {
          recovered = true;
          const fresh = JSON.parse(session.snapshot()).checkpoint as Record<
            string,
            unknown
          >;
          // 손상된 runtime 메타만 복구하고 유효한 history sequence는 보존
          for (const key of [
            'historyRevision',
            'historyEpoch',
            'historyStatusSeq',
          ]) {
            const value = checkpoint[key];
            if (
              typeof value === 'number' &&
              Number.isSafeInteger(value) &&
              value >= 0 &&
              value < Number.MAX_SAFE_INTEGER
            )
              fresh[key] = value;
          }
          session.restore_checkpoint(JSON.stringify(fresh));
        }
      }
      return {
        recovered,
        snapshot: () => parse(() => session.snapshot()),
        read: (command, args) =>
          parse(() => session.read(command, JSON.stringify(args))),
        prepare: (command, args, token) =>
          parse(() =>
            session.prepare_command(command, JSON.stringify(args), token),
          ),
        confirm: (token) => parse(() => session.confirm_command(token)),
        discard: (token) => session.discard_command(token),
        dispose: () => session.free(),
      };
    },
    kind: (command) => wasm.web_command_kind(command),
    keyboard(mappings, mode) {
      const matcher = new wasm.WebKeyboardMatcher(
        JSON.stringify(mappings),
        mode,
      );
      return {
        update: (mappings, mode) => {
          matcher.update(JSON.stringify(mappings), mode);
        },
        feed: (physicalId, candidates, pressed, device = 'keyboard') =>
          parse(() =>
            matcher.feed(
              JSON.stringify({
                physicalId,
                device,
                candidates,
                isDown: pressed,
              }),
            ),
          ),
        active: () => parse(() => matcher.active()),
        clear: () => matcher.clear(),
        dispose: () => matcher.free(),
      };
    },
  };
};
