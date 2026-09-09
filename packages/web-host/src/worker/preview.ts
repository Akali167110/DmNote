import type { WebRole } from '../protocol';
import type { PreviewEnvelope, PreviewValidation } from './previewValidation';

interface Registration {
  role: WebRole;
  generation: number;
  callbackId: number;
  index: number;
}
interface Session {
  owner: string;
  role: WebRole;
  generation: number;
  lastSeq: number;
}
export type PreviewSend = (
  clientId: string,
  callbackId: number,
  payload:
    | { index: number; message: PreviewEnvelope }
    | { index: number; end: true },
) => void;

/** 문서별 인스턴스. worker 명령 큐가 admission·commit 순서의 소유자 */
export function createPreviewBroker(
  send: PreviewSend,
  validation: PreviewValidation,
) {
  let generation = 0;
  const channels = new Map<string, Registration>();
  const sessions = new Map<string, Session>();
  const tombstones = new Set<string>();
  const tombstone = (id: string) => {
    tombstones.add(id);
    if (tombstones.size > 1024)
      tombstones.delete(tombstones.values().next().value!);
  };
  const cancellation = (id: string, session: Session): PreviewEnvelope => ({
    schemaVersion: 1,
    sessionId: id,
    seq: Math.min(Number.MAX_SAFE_INTEGER, session.lastSeq + 1),
    kind: 'cancel',
    sourceLabel: session.role,
    domain: 'keyPosition',
    mode: '',
    targets: [],
    patch: {},
  });
  const broadcast = (envelope: PreviewEnvelope, exclude?: string) => {
    for (const [clientId, registration] of channels) {
      if (clientId === exclude) continue;
      try {
        send(clientId, registration.callbackId, {
          index: registration.index++,
          message: envelope,
        });
      } catch (error) {
        console.warn('Editor preview delivery failed', error);
      }
    }
  };
  const closeChannel = (clientId: string) => {
    const registration = channels.get(clientId);
    if (!registration) return;
    try {
      send(clientId, registration.callbackId, {
        index: registration.index,
        end: true,
      });
    } catch (error) {
      console.warn('Editor preview channel close failed', error);
    }
    channels.delete(clientId);
  };
  const end = (
    id: string,
    session: Session,
    exclude?: string,
    notify = true,
  ) => {
    sessions.delete(id);
    tombstone(id);
    if (notify) broadcast(cancellation(id, session), exclude);
  };
  const registrationFor = (clientId: string, role: WebRole) => {
    const registration = channels.get(clientId);
    if (!registration || registration.role !== role)
      throw new Error('preview publisher is not subscribed');
    return registration;
  };
  return {
    subscribe(clientId: string, role: WebRole, channel: unknown): number {
      if (typeof channel !== 'string' || !/^__CHANNEL__:\d+$/.test(channel))
        throw new Error('invalid preview Channel');
      const callbackId = Number(channel.slice('__CHANNEL__:'.length));
      if (!Number.isSafeInteger(callbackId) || callbackId > 0xffffffff)
        throw new Error('invalid preview Channel');
      if (!Number.isSafeInteger(generation + 1))
        throw new Error('preview registration generation overflow');
      closeChannel(clientId);
      channels.set(clientId, {
        role,
        generation: ++generation,
        callbackId,
        index: 0,
      });
      for (const [id, session] of sessions)
        if (session.owner === clientId) end(id, session);
      return generation;
    },
    publish(clientId: string, role: WebRole, request: unknown): void {
      const envelope = validation.validatePublish(request, role);
      if (tombstones.has(envelope.sessionId))
        throw new Error('preview session has already ended');
      const registration = registrationFor(clientId, role);
      const session = sessions.get(envelope.sessionId);
      if (session) {
        if (session.owner !== clientId)
          throw new Error('preview session is owned by another window');
        if (session.generation !== registration.generation)
          throw new Error('preview session registration is stale');
        if (envelope.seq <= session.lastSeq)
          throw new Error('preview sequence must increase monotonically');
        session.lastSeq = envelope.seq;
      } else {
        if (sessions.size >= 1024)
          throw new Error('active preview session count exceeds 1024');
        sessions.set(envelope.sessionId, {
          owner: clientId,
          role,
          generation: registration.generation,
          lastSeq: envelope.seq,
        });
      }
      broadcast(envelope, clientId);
    },
    cancel(clientId: string, role: WebRole, sessionId: unknown): void {
      if (typeof sessionId !== 'string' || !validation.isSessionId(sessionId))
        throw new Error('preview sessionId must be a UUID');
      if (tombstones.has(sessionId))
        throw new Error('preview session has already ended');
      const session = sessions.get(sessionId);
      if (session && session.owner !== clientId)
        throw new Error('preview session is owned by another window');
      const resolved = session ?? {
        owner: clientId,
        role,
        generation: registrationFor(clientId, role).generation,
        lastSeq: 0,
      };
      end(sessionId, resolved, clientId);
    },
    finishCommittedSession(
      clientId: string,
      role: WebRole,
      sessionId: unknown,
      broadcastCancel: boolean,
    ): boolean {
      if (
        typeof sessionId !== 'string' ||
        !validation.isSessionId(sessionId) ||
        tombstones.has(sessionId)
      )
        return false;
      const session = sessions.get(sessionId);
      if (session && session.owner !== clientId)
        throw new Error('preview session is owned by another window');
      end(
        sessionId,
        session ?? {
          owner: clientId,
          role,
          generation: channels.get(clientId)?.generation ?? 0,
          lastSeq: 0,
        },
        clientId,
        broadcastCancel,
      );
      return true;
    },
    cancelAll(): number {
      const count = sessions.size;
      for (const [id, session] of sessions) end(id, session);
      return count;
    },
    disconnect(clientId: string): void {
      closeChannel(clientId);
      for (const [id, session] of sessions)
        if (session.owner === clientId) end(id, session);
    },
  };
}
