import { describe, expect, it } from 'vitest';
import { attachBrowserKeyboard } from './keyboard';
import type { WebClientMessage } from '../protocol';

describe('브라우저 키 입력 소유권', () => {
  it('현재 앱 키 매핑을 사용하고 반복 keydown과 미소유 keyup을 제외한다', () => {
    const events: WebClientMessage[] = [];
    const input = attachBrowserKeyboard((event) => events.push(event));
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyA', key: 'a' }),
    );
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyA', key: 'a', repeat: true }),
    );
    window.dispatchEvent(
      new KeyboardEvent('keyup', { code: 'KeyB', key: 'b' }),
    );
    window.dispatchEvent(
      new KeyboardEvent('keyup', { code: 'KeyA', key: 'A' }),
    );
    expect(events).toMatchObject([
      { type: 'input', globalKey: 'A', pressed: true },
      { type: 'input', globalKey: 'A', pressed: false },
    ]);
    input.dispose();
  });

  it('blur와 해제에서 모든 눌린 키를 한 번씩 해제한다', () => {
    const events: WebClientMessage[] = [];
    const input = attachBrowserKeyboard((event) => events.push(event));
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'ShiftLeft', key: 'Shift' }),
    );
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyC', key: 'c' }),
    );
    window.dispatchEvent(new Event('blur'));
    input.dispose();
    expect(events).toHaveLength(4);
    expect(events.slice(2)).toMatchObject([
      { globalKey: 'LEFT SHIFT', pressed: false },
      { globalKey: 'C', pressed: false },
    ]);
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyD', key: 'd' }),
    );
    expect(events).toHaveLength(4);
  });
  it('mouse buttons preserve native labels, chord changes and blur releases without canceling clicks', () => {
    const events: WebClientMessage[] = [];
    const input = attachBrowserKeyboard((event) => events.push(event));
    const pointer = (
      type: string,
      button: number,
      buttons: number,
      pointerType = 'mouse',
    ) => {
      const event = new MouseEvent(type, { button, buttons, cancelable: true });
      Object.defineProperty(event, 'pointerType', { value: pointerType });
      window.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    };
    pointer('pointerdown', 0, 1, 'touch');
    pointer('pointerdown', 0, 1, 'pen');
    expect(events).toHaveLength(0);
    pointer('pointerdown', 0, 1);
    pointer('pointermove', 2, 3);
    pointer('pointermove', 0, 2);
    pointer('pointerup', 2, 0);
    expect(events).toMatchObject([
      { device: 'mouse', globalKey: 'MOUSE1', pressed: true },
      { device: 'mouse', globalKey: 'MOUSE2', pressed: true },
      { device: 'mouse', globalKey: 'MOUSE1', pressed: false },
      { device: 'mouse', globalKey: 'MOUSE2', pressed: false },
    ]);
    pointer('pointerdown', 1, 4);
    pointer('pointermove', 3, 12);
    pointer('pointermove', 4, 28);
    window.dispatchEvent(new Event('blur'));
    expect(events.slice(4)).toMatchObject([
      { globalKey: 'MOUSE3', pressed: true },
      { globalKey: 'MOUSE4', pressed: true },
      { globalKey: 'MOUSE5', pressed: true },
      { globalKey: 'MOUSE3', pressed: false },
      { globalKey: 'MOUSE4', pressed: false },
      { globalKey: 'MOUSE5', pressed: false },
    ]);
    input.dispose();
    pointer('pointerdown', 0, 1);
    expect(events).toHaveLength(10);
  });
});
