import type { WebClientMessage } from '../protocol';
import type { Client, Session, InvokeEngine } from './types';
import { send, broadcast } from './wire';

export const dispatchInput = async (
  session: Session,
  client: Client,
  message: Extract<WebClientMessage, { type: 'input' }>,
  invokeEngine: InvokeEngine,
  randomId: () => string,
) => {
  const physicalId = `${client.id}:${message.code}:${message.location}`;
  const candidates = message.globalKey
    ? [message.globalKey]
    : [message.code, message.key];
  if (message.pressed)
    client.keys.set(physicalId, { candidates, downAt: message.timestamp });
  const held = client.keys.get(physicalId);
  if (!message.pressed) client.keys.delete(physicalId);
  const match = session.keyboard.feed(
    physicalId,
    candidates,
    message.pressed,
    message.device,
  );
  const label = message.globalKey ?? message.key;
  for (const subscriber of session.clients.values())
    if (subscriber.rawSubscriptions)
      send(subscriber, {
        type: 'event',
        event: 'input:raw',
        payload: {
          label,
          labels: candidates,
          state: message.pressed ? 'DOWN' : 'UP',
          device: message.device ?? 'keyboard',
        },
      });
  if (!match) return;
  if (match.pressedLabel)
    broadcast(session, {
      type: 'event',
      event: 'input:press',
      payload: { label: match.pressedLabel, mode: match.mode },
    });
  for (const event of match.events)
    if (event.transition !== null)
      broadcast(session, {
        type: 'event',
        event: 'keys:state',
        payload: {
          key: event.canonical,
          state: event.transition ? 'DOWN' : 'UP',
          mode: match.mode,
          eventAgeMs: 0,
          ...(!event.transition && event.canUsePhysicalHoldDuration && held
            ? { holdDurationMs: Math.max(0, message.timestamp - held.downAt) }
            : {}),
        },
      });
  const pressedEvents = match.events.filter((event) => event.press);
  const indices = [
    ...new Set(pressedEvents.flatMap((event) => event.slotIndices)),
  ].sort((a, b) => a - b);
  const store = session.engine.snapshot().store;
  const positions =
    (
      store.keyPositions as Record<
        string,
        Array<{
          soundEnabled?: boolean;
          soundPath?: string;
          soundVolume?: number;
        }>
      >
    )?.[match.mode] ?? [];
  const sound = indices
    .map((index) => positions[index])
    .find((position) => position?.soundEnabled && position.soundPath);
  if (message.pressed && message.device !== 'mouse' && sound)
    send(client, {
      type: 'event',
      event: 'web:input-sound',
      payload: {
        soundPath: sound.soundPath,
        volume: Math.max(0, Math.min(2, (sound.soundVolume ?? 100) / 100)),
      },
    });
  const pressed = pressedEvents.map((event) => event.canonical);
  if (pressed.length)
    await invokeEngine(session, client, {
      type: 'invoke',
      requestId: randomId(),
      command: 'web_input_press',
      args: { mode: match.mode, keys: pressed },
    });
};

export const releaseClientInput = (session: Session, client: Client): void => {
  for (const [physicalId, held] of client.keys) {
    const match = session.keyboard.feed(physicalId, held.candidates, false);
    if (match)
      for (const event of match.events)
        if (event.transition === false)
          broadcast(session, {
            type: 'event',
            event: 'keys:state',
            payload: {
              key: event.canonical,
              state: 'UP',
              mode: match.mode,
              eventAgeMs: 0,
            },
          });
  }
};
