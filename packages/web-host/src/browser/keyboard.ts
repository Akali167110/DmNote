import { getKeyInfo, getKeyInfoByGlobalKey } from '@dmnote/editor/model';
import type { WebClientMessage } from '../protocol';

type BrowserInputMessage = Extract<WebClientMessage, { type: 'input' }>;

/** 반복 keydown 중복 제거, 포커스 이탈 시 이 화면이 누른 키만 해제 */
export function attachBrowserKeyboard(
  send: (message: BrowserInputMessage) => void,
  target: Window = window,
): { releaseAll(): void; dispose(): void } {
  const pressed = new Map<string, BrowserInputMessage>();
  const releaseAll = () => {
    const active = [...pressed.values()];
    pressed.clear();
    for (const message of active)
      send({ ...message, pressed: false, timestamp: Date.now() });
  };
  const keydown = (event: KeyboardEvent) => {
    if (event.repeat || event.isComposing || pressed.has(event.code)) return;
    const globalKey = getKeyInfo(event.code, event.key).globalKey;
    if (!globalKey || event.key === 'Unidentified') return;
    const message: BrowserInputMessage = {
      type: 'input',
      code: event.code,
      key: event.key,
      location: event.location,
      pressed: true,
      timestamp: Date.now(),
      globalKey,
      device: 'keyboard',
    };
    pressed.set(event.code, message);
    send(message);
  };
  const keyup = (event: KeyboardEvent) => {
    const message = pressed.get(event.code);
    if (!message) return;
    pressed.delete(event.code);
    send({ ...message, pressed: false, timestamp: Date.now() });
  };
  const mouseButtons = ['MOUSE1', 'MOUSE3', 'MOUSE2', 'MOUSE4', 'MOUSE5'];
  const mouseMasks = [1, 4, 2, 8, 16];
  const setMouseButton = (button: number, isDown: boolean) => {
    const globalKey = mouseButtons[button];
    if (!globalKey) return;
    const keyInfo = getKeyInfoByGlobalKey(globalKey);
    const code = keyInfo.browserKey;
    const current = pressed.get(code);
    if (!!current === isDown) return;
    if (!isDown) {
      pressed.delete(code);
      send({ ...current!, pressed: false, timestamp: Date.now() });
      return;
    }
    const message: BrowserInputMessage = {
      type: 'input',
      code,
      key: globalKey,
      globalKey,
      location: 0,
      device: 'mouse',
      pressed: true,
      timestamp: Date.now(),
    };
    pressed.set(code, message);
    send(message);
  };
  const releaseMouse = () => {
    for (let button = 0; button < mouseButtons.length; button += 1)
      setMouseButton(button, false);
  };
  const pointerdown = (event: PointerEvent) => {
    if (event.pointerType === 'mouse') setMouseButton(event.button, true);
  };
  const pointerup = (event: PointerEvent) => {
    if (event.pointerType === 'mouse') releaseMouse();
  };
  const pointermove = (event: PointerEvent) => {
    if (
      event.pointerType !== 'mouse' ||
      ![...pressed.values()].some((message) => message.device === 'mouse')
    )
      return;
    // 추가 버튼 전환은 pointerdown/up 대신 pointermove의 buttons로 전달됨
    for (let button = 0; button < mouseButtons.length; button += 1)
      setMouseButton(button, (event.buttons & mouseMasks[button]) !== 0);
  };
  const visibility = () => {
    if (target.document.visibilityState === 'hidden') releaseAll();
  };
  target.addEventListener('keydown', keydown, true);
  target.addEventListener('keyup', keyup, true);
  target.addEventListener('pointerdown', pointerdown, true);
  target.addEventListener('pointerup', pointerup, true);
  target.addEventListener('pointercancel', pointerup, true);
  target.addEventListener('pointermove', pointermove, true);
  target.addEventListener('blur', releaseAll);
  target.document.addEventListener('visibilitychange', visibility);
  return {
    releaseAll,
    dispose() {
      releaseAll();
      target.removeEventListener('keydown', keydown, true);
      target.removeEventListener('keyup', keyup, true);
      target.removeEventListener('pointerdown', pointerdown, true);
      target.removeEventListener('pointerup', pointerup, true);
      target.removeEventListener('pointercancel', pointerup, true);
      target.removeEventListener('pointermove', pointermove, true);
      target.removeEventListener('blur', releaseAll);
      target.document.removeEventListener('visibilitychange', visibility);
    },
  };
}
