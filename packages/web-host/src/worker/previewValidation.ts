export interface PreviewEnvelope {
  schemaVersion: 1;
  sessionId: string;
  seq: number;
  kind: 'patch' | 'cancel';
  sourceLabel: string;
  domain:
    | 'keyPosition'
    | 'statPosition'
    | 'graphPosition'
    | 'knobPosition'
    | 'spritePosition';
  mode: string;
  targets: number[];
  patch: Record<string, unknown>;
}

/** Rust와 native가 공유하는 wire 검증 주입 */
export interface PreviewValidation {
  validatePublish(request: unknown, sourceLabel: string): PreviewEnvelope;
  isSessionId(value: string): boolean;
}
