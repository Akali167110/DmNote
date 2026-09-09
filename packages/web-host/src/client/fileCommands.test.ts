import { describe, expect, it, vi } from 'vitest';
import { prepareFileCommand } from './fileCommands';

describe('파일 명령 전처리', () => {
  it('프리셋 파일을 해석하고 기존 커맨드 인자는 유지한다', async () => {
    const picker = vi.fn().mockResolvedValue([
      {
        name: 'a.json',
        mimeType: 'application/json',
        bytes: new TextEncoder().encode('{"keys":{}}').buffer,
      },
    ]);
    const result = await prepareFileCommand(
      'preset_load_tab',
      { extra: 1 },
      picker,
      new AbortController().signal,
    );
    expect(result).toEqual({
      cancelled: false,
      args: { extra: 1, preset: { keys: {} } },
    });
  });
  it('탭 CSS 취소 응답은 tabId를 유지하고 JS는 복수 파일을 요청한다', async () => {
    const picker = vi.fn().mockResolvedValue([]);
    expect(
      await prepareFileCommand(
        'css_tab_load',
        { tabId: 'tab' },
        picker,
        new AbortController().signal,
      ),
    ).toEqual({ cancelled: true, result: { success: false, tabId: 'tab' } });
    await prepareFileCommand(
      'js_load',
      {},
      picker,
      new AbortController().signal,
    );
    expect(picker.mock.calls[1][0]).toMatchObject({ multiple: true });
  });
});
