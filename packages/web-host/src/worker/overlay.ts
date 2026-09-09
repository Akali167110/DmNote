import type {
  WebOverlayAnchor,
  WebOverlayBounds,
  WebOverlayState,
} from '../browser/overlay';
import type { WebEvent } from '../protocol';

const anchors = new Set<WebOverlayAnchor>([
  'top-left',
  'top-right',
  'bottom-left',
  'bottom-right',
  'center',
  'fixed-position',
]);
const finite = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);
const dimension = (value: unknown): number => {
  if (!finite(value))
    throw new TypeError('Overlay dimensions must be finite numbers');
  return Math.round(Math.min(4096, Math.max(100, value)));
};
const anchorValue = (
  value: unknown,
  fallback: WebOverlayAnchor,
): WebOverlayAnchor =>
  typeof value === 'string' && anchors.has(value as WebOverlayAnchor)
    ? (value as WebOverlayAnchor)
    : fallback;

export const createWebOverlayPreview = () => {
  let state: WebOverlayState = {
    bounds: { x: -430, y: -160, width: 860, height: 320 },
    visible: true,
    locked: false,
    anchor: 'top-left',
    opacity: 1,
    fadeDurationMs: 0,
  };
  let lastLeft: number | undefined;
  let lastTop: number | undefined;
  const snapshot = (): WebOverlayState => ({
    ...state,
    bounds: { ...state.bounds },
  });
  const event = (): WebEvent => ({
    event: 'web:overlay-state',
    payload: snapshot(),
  });
  const sync = (overlay: {
    visible?: boolean;
    locked?: boolean;
    anchor?: string;
  }): WebOverlayState => {
    state = {
      ...state,
      ...(overlay.visible === undefined ? {} : { visible: overlay.visible }),
      ...(overlay.locked === undefined ? {} : { locked: overlay.locked }),
      anchor: anchorValue(overlay.anchor, state.anchor),
    };
    return snapshot();
  };
  const invoke = (
    command: string,
    args: Record<string, unknown>,
  ): { result: WebOverlayBounds | boolean; events: WebEvent[] } | undefined => {
    if (command === 'overlay_transition_fade') {
      if (
        !finite(args.alpha) ||
        !finite(args.durationMs) ||
        args.durationMs < 0
      )
        throw new TypeError('Invalid overlay fade');
      state = {
        ...state,
        opacity: Math.min(1, Math.max(0, args.alpha)),
        fadeDurationMs: args.durationMs,
      };
      return { result: true, events: [event()] };
    }
    if (command === 'overlay_reset_position') {
      state = {
        ...state,
        bounds: {
          ...state.bounds,
          x: -state.bounds.width / 2,
          y: -state.bounds.height / 2,
        },
      };
      return {
        result: { ...state.bounds },
        events: [
          { event: 'overlay:resized', payload: { ...state.bounds } },
          event(),
        ],
      };
    }
    if (command !== 'overlay_resize') return;
    const payload = args.payload;
    if (!payload || typeof payload !== 'object')
      throw new TypeError('Overlay resize payload is required');
    const input = payload as Record<string, unknown>;
    const width = dimension(input.width);
    const height = dimension(input.height);
    const anchor = anchorValue(input.anchor, state.anchor);
    const previous = state.bounds;
    let { x, y } = previous;
    if (anchor === 'top-right' || anchor === 'bottom-right')
      x += previous.width - width;
    if (anchor === 'bottom-left' || anchor === 'bottom-right')
      y += previous.height - height;
    if (anchor === 'center') {
      x += (previous.width - width) / 2;
      y += (previous.height - height) / 2;
    }
    if (anchor === 'fixed-position') {
      if (finite(input.fixedPositionDeltaX)) x += input.fixedPositionDeltaX;
      if (finite(input.fixedPositionDeltaY)) y += input.fixedPositionDeltaY;
    }
    if (finite(input.contentLeftOffset)) {
      const delta =
        input.contentLeftOffset - (lastLeft ?? input.contentLeftOffset);
      if (anchor === 'center') x -= delta / 2;
      else if (anchor !== 'top-right' && anchor !== 'bottom-right') x -= delta;
      lastLeft = input.contentLeftOffset;
    }
    if (finite(input.contentTopOffset)) {
      const delta =
        input.contentTopOffset - (lastTop ?? input.contentTopOffset);
      if (anchor === 'center') y -= delta / 2;
      else if (anchor !== 'bottom-left' && anchor !== 'bottom-right')
        y -= delta;
      lastTop = input.contentTopOffset;
    }
    state = { ...state, bounds: { x, y, width, height } };
    return {
      result: { ...state.bounds },
      events: [
        { event: 'overlay:resized', payload: { ...state.bounds } },
        event(),
      ],
    };
  };
  return { snapshot, sync, invoke };
};
