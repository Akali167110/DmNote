import { describe, expect, it, vi } from 'vitest';
import { createWebAudioPreview } from './audio';
const setup = () => {
  const sources: Array<{
    start: ReturnType<typeof vi.fn>;
    stop: ReturnType<typeof vi.fn>;
    disconnect: ReturnType<typeof vi.fn>;
    connect: ReturnType<typeof vi.fn>;
    buffer: AudioBuffer | null;
    onended: (() => void) | null;
  }> = [];
  const gains: Array<{
    gain: { value: number };
    connect: ReturnType<typeof vi.fn>;
    disconnect: ReturnType<typeof vi.fn>;
  }> = [];
  const decoded = {} as AudioBuffer;
  const context = {
    state: 'suspended',
    destination: {},
    resume: vi.fn(async () => {
      context.state = 'running';
    }),
    close: vi.fn(async () => {
      context.state = 'closed';
    }),
    decodeAudioData: vi.fn(async () => decoded),
    createBufferSource: vi.fn(() => {
      const source = {
        start: vi.fn(),
        stop: vi.fn(),
        disconnect: vi.fn(),
        connect: vi.fn(),
        buffer: null as AudioBuffer | null,
        onended: null as (() => void) | null,
      };
      sources.push(source);
      return source;
    }),
    createGain: vi.fn(() => {
      const output = {
        gain: { value: 1 },
        connect: vi.fn(),
        disconnect: vi.fn(),
      };
      gains.push(output);
      return output;
    }),
  };
  const loadBytes = vi.fn(async () => new ArrayBuffer(8));
  const resolveAsset = vi.fn(() => 'blob:sound');
  const preview = createWebAudioPreview({
    resolveAsset,
    loadBytes,
    createContext: () => context as unknown as AudioContext,
  });
  return { preview, context, sources, gains, loadBytes, resolveAsset };
};
describe('브라우저 키음 재생', () => {
  it('한 자산을 한 번 decode하고 press마다 독립 source와 키별 볼륨을 사용한다', async () => {
    const test = setup();
    await test.preview.unlock();
    await Promise.all([
      test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 0.4 }),
      test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 3 }),
    ]);
    expect(test.context.decodeAudioData).toHaveBeenCalledTimes(1);
    expect(test.loadBytes).toHaveBeenCalledTimes(1);
    expect(test.sources).toHaveLength(2);
    expect(test.gains.map((gain) => gain.gain.value)).toEqual([0.4, 2]);
    expect(
      test.sources.every(
        (source) =>
          source.start.mock.calls.length === 1 &&
          source.stop.mock.calls.length === 0,
      ),
    ).toBe(true);
    test.preview.dispose();
    expect(
      test.sources.every((source) => source.stop.mock.calls.length === 1),
    ).toBe(true);
    expect(test.context.close).toHaveBeenCalledTimes(1);
  });
  it('교체된 WAV를 다시 decode하고 삭제 중 완료되는 오래된 파일은 재생하지 않는다', async () => {
    const test = setup();
    await test.preview.unlock();
    await test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 1 });
    test.resolveAsset.mockReturnValue('blob:edited');
    await test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 1 });
    expect(test.context.decodeAudioData).toHaveBeenCalledTimes(2);
    let release!: (bytes: ArrayBuffer) => void;
    test.loadBytes.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const pending = test.preview.play({
      soundPath: '/assets/sounds/pending.wav',
      volume: 1,
    });
    test.preview.invalidate(['/assets/sounds/pending.wav']);
    release(new ArrayBuffer(4));
    await pending;
    expect(test.sources).toHaveLength(2);
    test.preview.dispose();
  });
  it('사용자 활성화 전 입력을 쌓지 않으며 mute와 dispose 뒤에는 재생하지 않는다', async () => {
    const test = setup();
    await expect(
      test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 1 }),
    ).rejects.toThrow('user gesture');
    await test.preview.unlock();
    await test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 0 });
    test.preview.dispose();
    await test.preview.play({ soundPath: '/assets/sounds/a.wav', volume: 1 });
    expect(test.sources).toHaveLength(0);
    expect(test.loadBytes).not.toHaveBeenCalled();
  });
});
