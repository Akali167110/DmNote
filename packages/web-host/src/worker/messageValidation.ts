import type { WebClientMessage } from '../protocol';

const record = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, limit: number): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= limit;
const files = (value: unknown): boolean =>
  value === undefined ||
  (Array.isArray(value) &&
    value.length <= 100 &&
    value.every(
      (file) =>
        record(file) &&
        text(file.name, 1024) &&
        typeof file.mimeType === 'string' &&
        file.mimeType.length <= 256 &&
        file.bytes instanceof ArrayBuffer,
    ));
export const isClientMessage = (value: unknown): value is WebClientMessage => {
  if (!record(value)) return false;
  switch (value.type) {
    case 'connect':
      return (
        typeof value.version === 'number' &&
        text(value.requestId, 256) &&
        text(value.documentId, 200) &&
        text(value.clientId, 200) &&
        text(value.realmId, 200) &&
        (value.role === 'main' || value.role === 'overlay')
      );
    case 'invoke':
      return (
        text(value.requestId, 256) &&
        text(value.command, 128) &&
        record(value.args) &&
        files(value.files)
      );
    case 'emit':
      return text(value.requestId, 256) && text(value.event, 256);
    case 'input':
      return (
        (value.device === undefined ||
          value.device === 'keyboard' ||
          value.device === 'mouse') &&
        text(value.code, 128) &&
        typeof value.key === 'string' &&
        value.key.length <= 128 &&
        Number.isInteger(value.location) &&
        Number(value.location) >= 0 &&
        Number(value.location) <= 3 &&
        typeof value.pressed === 'boolean' &&
        typeof value.timestamp === 'number' &&
        Number.isFinite(value.timestamp) &&
        (value.globalKey === undefined || text(value.globalKey, 128))
      );
    case 'disconnect':
      return true;
    default:
      return false;
  }
};
