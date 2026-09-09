import { describe, expect, it } from 'vitest';
import { createWebOverlayPreview } from './overlay';
import { applyWebOverlayState } from '../browser/overlay';

describe('웹 오버레이 미리보기 프레임', () => {
  it('앱 크기 제한과 anchor, content offset을 로컬 좌표에 적용한다', () => {
    const overlay = createWebOverlayPreview();
    overlay.sync({ anchor: 'bottom-right' });
    const resized = overlay.invoke('overlay_resize', {
      payload: { width: 1000, height: 500 },
    });
    expect(resized?.result).toEqual({
      x: -570,
      y: -340,
      width: 1000,
      height: 500,
    });
    overlay.sync({ anchor: 'fixed-position' });
    overlay.invoke('overlay_resize', {
      payload: { width: 1000, height: 500, contentLeftOffset: 20 },
    });
    const offset = overlay.invoke('overlay_resize', {
      payload: {
        width: 5000,
        height: 10,
        contentLeftOffset: 40,
        fixedPositionDeltaX: 5,
      },
    });
    expect(offset?.result).toEqual({
      x: -585,
      y: -340,
      width: 4096,
      height: 100,
    });
    expect(overlay.invoke('overlay_reset_position', {})?.result).toEqual({
      x: -2048,
      y: -50,
      width: 4096,
      height: 100,
    });
  });
  it('페이드와 표시·잠금을 실제 iframe style에 전달한다', () => {
    const overlay = createWebOverlayPreview();
    overlay.sync({ visible: false, locked: true });
    expect(
      overlay.invoke('overlay_transition_fade', { alpha: 0.4, durationMs: 120 })
        ?.result,
    ).toBe(true);
    const frame = document.createElement('iframe');
    applyWebOverlayState(frame, overlay.snapshot());
    expect(frame.style.display).toBe('none');
    expect(frame.style.opacity).toBe('0.4');
    expect(frame.style.transitionDuration).toBe('120ms');
    expect(frame.style.pointerEvents).toBe('none');
    overlay.sync({ visible: true, locked: false, anchor: 'not-valid' });
    applyWebOverlayState(frame, overlay.snapshot());
    expect(frame.style.display).toBe('');
    expect(frame.style.pointerEvents).toBe('auto');
    expect(overlay.snapshot().anchor).toBe('top-left');
  });
  it('잘못된 치수는 변경 전에 거절하고 스냅샷을 외부 변경에서 분리한다', () => {
    const overlay = createWebOverlayPreview();
    const before = overlay.snapshot();
    expect(() =>
      overlay.invoke('overlay_resize', {
        payload: { width: Number.NaN, height: 500 },
      }),
    ).toThrow();
    expect(overlay.snapshot()).toEqual(before);
    before.bounds.width = 100;
    expect(overlay.snapshot().bounds.width).toBe(860);
  });
});
