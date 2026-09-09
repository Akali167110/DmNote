import { webHostError, type WebServerMessage } from '../protocol';
import type { Client, Session, Invoke, InvokeEngine } from './types';
import { targetRole, broadcast } from './wire';

export const createCommandRouter =
  (invokeEngine: InvokeEngine) =>
  async (
    session: Session,
    client: Client,
    message: Invoke,
  ): Promise<{
    result: unknown;
    download?: Extract<
      WebServerMessage,
      { type: 'response'; ok: true }
    >['download'];
  }> => {
    const { command, args } = message;
    if (command === 'web_main_disconnected' || command === 'web_input_press')
      throw webHostError(
        'UNSUPPORTED_WEB_COMMAND',
        'This command is reserved for the worker host',
      );
    const local = session.overlay.invoke(command, args);
    if (local) {
      for (const event of local.events)
        broadcast(session, { type: 'event', ...event });
      return { result: local.result };
    }
    if (
      command === 'plugin_bridge_send' ||
      command === 'plugin_bridge_send_to'
    ) {
      if (typeof args.messageType !== 'string')
        throw new TypeError('messageType must be a string');
      const role =
        command === 'plugin_bridge_send_to'
          ? targetRole(args.target)
          : undefined;
      if (
        role &&
        ![...session.clients.values()].some((client) => client.role === role)
      )
        throw `Window '${role}' not found`;
      broadcast(session, {
        type: 'event',
        event: 'plugin-bridge:message',
        payload: {
          type: args.messageType,
          data: args.data ?? null,
          ...(role ? { target: role } : {}),
        },
      });
      return { result: null };
    }
    if (command === 'app_quit_after_editor_flush') {
      session.flush.acknowledge(client.id, args.handshakeId);
      return { result: null };
    }
    if (command === 'app_cancel_editor_flush') {
      session.flush.cancel(client.id, args.handshakeId);
      return { result: null };
    }
    if (
      command === 'raw_input_subscribe' ||
      command === 'raw_input_unsubscribe'
    ) {
      const receipt = client.rawReceipts.get(message.requestId);
      if (receipt) {
        if (receipt.command !== command)
          throw webHostError(
            'REQUEST_ID_REUSED',
            'Raw input request ID reused',
          );
        return { result: { count: receipt.count } };
      }
      client.rawSubscriptions = Math.max(
        0,
        client.rawSubscriptions + (command === 'raw_input_subscribe' ? 1 : -1),
      );
      client.rawReceipts.set(message.requestId, {
        command,
        count: client.rawSubscriptions,
      });
      return { result: { count: client.rawSubscriptions } };
    }
    if (command === 'editor_preview_subscribe')
      return {
        result: session.preview.subscribe(client.id, client.role, args.channel),
      };
    if (command === 'editor_preview_publish')
      return {
        result: session.preview.publish(client.id, client.role, args.request),
      };
    if (command === 'editor_preview_cancel')
      return {
        result: session.preview.cancel(client.id, client.role, args.sessionId),
      };
    const result = await invokeEngine(session, client, message);
    if (
      command === 'css_tab_export' &&
      result &&
      typeof result === 'object' &&
      'content' in result
    )
      return {
        result: { success: true, path: 'path' in result ? result.path : null },
        download: {
          name: 'dmnote-tab.css',
          mimeType: 'text/css',
          bytes: new TextEncoder().encode(String(result.content)).buffer,
        },
      };
    if (command === 'preset_save' || command === 'preset_save_tab')
      return {
        result: { success: true },
        download: {
          name:
            command === 'preset_save_tab'
              ? 'dmnote-tab.json'
              : 'dmnote-preset.json',
          mimeType: 'application/json',
          bytes: new TextEncoder().encode(JSON.stringify(result, null, 2))
            .buffer,
        },
      };
    return { result };
  };
