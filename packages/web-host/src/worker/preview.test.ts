import { describe, expect, it, vi } from 'vitest';
import { createPreviewBroker } from './preview';
import type { PreviewEnvelope } from './previewValidation';

const id = 'e7ac4b9f-867a-4d44-8351-cd7bf4218e17';
const request = (seq = 1) => ({
  schemaVersion: 1,
  sessionId: id,
  seq,
  domain: 'keyPosition',
  mode: '4key',
  targets: [0],
  patch: { dx: 3 },
});
const setup = () => {
  const send = vi.fn();
  const broker = createPreviewBroker(send, {
    validatePublish(value, sourceLabel) {
      return { ...(value as PreviewEnvelope), kind: 'patch', sourceLabel };
    },
    isSessionId: (value) => /^[\da-f-]{36}$/i.test(value),
  });
  broker.subscribe('a', 'main', '__CHANNEL__:1');
  broker.subscribe('b', 'overlay', '__CHANNEL__:2');
  return { broker, send };
};
describe('SharedWorker preview broker', () => {
  it('isolates source and owner, enforces monotonic sequence and closes sessions', () => {
    const { broker, send } = setup();
    broker.publish('a', 'main', request());
    expect(send).toHaveBeenLastCalledWith('b', 2, {
      index: 0,
      message: { ...request(), kind: 'patch', sourceLabel: 'main' },
    });
    expect(() => broker.publish('a', 'main', request())).toThrow(
      'monotonically',
    );
    expect(() => broker.publish('b', 'overlay', request(2))).toThrow(
      'another window',
    );
    expect(() => broker.cancel('b', 'overlay', id)).toThrow('another window');
    broker.publish('a', 'main', request(2));
    broker.cancel('a', 'main', id);
    expect(send).toHaveBeenLastCalledWith(
      'b',
      2,
      expect.objectContaining({
        index: 2,
        message: expect.objectContaining({
          seq: 3,
          kind: 'cancel',
          sourceLabel: 'main',
        }),
      }),
    );
    expect(() => broker.publish('a', 'main', request(4))).toThrow(
      'already ended',
    );
  });
  it('cancels owned previews when subscription generation changes or client disconnects', () => {
    const { broker, send } = setup();
    broker.publish('a', 'main', request());
    broker.subscribe('a', 'main', '__CHANNEL__:3');
    expect(send).toHaveBeenCalledWith('a', 1, { index: 0, end: true });
    expect(send).toHaveBeenCalledWith(
      'a',
      3,
      expect.objectContaining({
        index: 0,
        message: expect.objectContaining({ kind: 'cancel' }),
      }),
    );
    expect(() => broker.publish('a', 'main', request(2))).toThrow(
      'already ended',
    );
    const second = { ...request(), sessionId: crypto.randomUUID() };
    broker.publish('a', 'main', second);
    send.mockClear();
    broker.disconnect('a');
    expect(send).toHaveBeenCalledTimes(2);
    expect(send.mock.calls[0]).toEqual(['a', 3, { index: 1, end: true }]);
    expect(send.mock.calls[1]?.[0]).toBe('b');
    expect(() =>
      broker.publish('a', 'main', {
        ...request(),
        sessionId: crypto.randomUUID(),
      }),
    ).toThrow('not subscribed');
  });
  it('ends committed previews without echo and cancels all previews before history apply', () => {
    const { broker, send } = setup();
    broker.publish('a', 'main', request());
    send.mockClear();
    expect(broker.finishCommittedSession('a', 'main', id, false)).toBe(true);
    expect(send).not.toHaveBeenCalled();
    expect(broker.finishCommittedSession('a', 'main', id, true)).toBe(false);
    broker.publish('b', 'overlay', {
      ...request(),
      sessionId: crypto.randomUUID(),
    });
    expect(broker.cancelAll()).toBe(1);
    expect(broker.cancelAll()).toBe(0);
  });
  it('delegates wire validation to the common Rust validator before mutating state', () => {
    const validatePublish = vi.fn(() => {
      throw new Error('invalid native preview payload');
    });
    const broker = createPreviewBroker(vi.fn(), {
      validatePublish,
      isSessionId: () => false,
    });
    expect(() => broker.publish('a', 'main', request())).toThrow(
      'invalid native preview payload',
    );
    expect(validatePublish).toHaveBeenCalledWith(request(), 'main');
    expect(() => broker.cancel('a', 'main', id)).toThrow('UUID');
    expect(broker.finishCommittedSession('a', 'main', id, false)).toBe(false);
  });
});
