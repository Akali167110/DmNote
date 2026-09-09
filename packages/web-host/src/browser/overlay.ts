export type WebOverlayAnchor =
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right'
  | 'center'
  | 'fixed-position';
export interface WebOverlayBounds {
  /** 미리보기 컨테이너 중심 기준 좌상단 좌표 */
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface WebOverlayState {
  bounds: WebOverlayBounds;
  visible: boolean;
  locked: boolean;
  anchor: WebOverlayAnchor;
  opacity: number;
  fadeDurationMs: number;
}

/** position:relative인 컨테이너의 iframe 또는 미리보기 wrapper에 적용한다. */
export const applyWebOverlayState = (
  element: HTMLElement,
  state: WebOverlayState,
): void => {
  Object.assign(element.style, {
    position: 'absolute',
    left: '50%',
    top: '50%',
    width: `${state.bounds.width}px`,
    height: `${state.bounds.height}px`,
    transform: `translate(${state.bounds.x}px, ${state.bounds.y}px)`,
    display: state.visible ? '' : 'none',
    opacity: String(state.opacity),
    transitionProperty: 'opacity',
    transitionDuration: `${state.fadeDurationMs}ms`,
    pointerEvents: state.locked ? 'none' : 'auto',
  });
};
