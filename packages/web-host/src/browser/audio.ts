export interface WebInputSound {
  soundPath: string;
  /** 키별 0..200% 볼륨을 0..2 gain으로 변환한 값 */
  volume: number;
}
export interface WebAudioPreviewOptions {
  resolveAsset(path: string): string;
  target?: Window;
  createContext?: () => AudioContext;
  loadBytes?: (url: string) => Promise<ArrayBuffer>;
  onError?: (error: unknown) => void;
}

/** 입력 소유 클라이언트에서만 생성. 슬롯 선택과 repeat 판정은 공통 Rust matcher가 소유한다. */
export const createWebAudioPreview = ({
  resolveAsset,
  target = window,
  createContext = () => new AudioContext({ latencyHint: 'interactive' }),
  loadBytes = async (url) => {
    const response = await fetch(url);
    if (!response.ok)
      throw new Error(`Unable to load key sound: ${response.status}`);
    return response.arrayBuffer();
  },
  onError,
}: WebAudioPreviewOptions) => {
  let context: AudioContext | null = null;
  let unlocking: Promise<void> | null = null;
  let disposed = false;
  const cache = new Map<
    string,
    { url: string; buffer: Promise<AudioBuffer> }
  >();
  const active = new Map<AudioBufferSourceNode, GainNode>();

  const unlock = async (): Promise<void> => {
    if (disposed) return;
    context ??= createContext();
    if (context.state === 'running') return;
    if (!unlocking) {
      const resume = context.resume();
      unlocking = resume;
      try {
        await resume;
      } finally {
        if (unlocking === resume) unlocking = null;
      }
    } else await unlocking;
  };
  const activate = (event: Event) => {
    if (!event.isTrusted) return;
    void unlock().catch((error: unknown) => onError?.(error));
  };
  target.addEventListener('pointerdown', activate, true);
  target.addEventListener('keydown', activate, true);

  const play = async ({ soundPath, volume }: WebInputSound): Promise<void> => {
    if (disposed) return;
    if (!soundPath.trim() || !Number.isFinite(volume))
      throw new TypeError('Invalid web input sound');
    const gain = Math.min(2, Math.max(0, volume));
    if (gain === 0) return;
    // worker 응답을 기다리는 동안 끝나는 사용자 활성화는 capture listener에서 확보한다.
    if (unlocking) await unlocking;
    const current = context;
    if (!current || current.state !== 'running')
      throw new Error('A user gesture is required to enable key sounds');
    const url = resolveAsset(soundPath);
    let entry = cache.get(soundPath);
    if (!entry || entry.url !== url) {
      const buffer = loadBytes(url).then((bytes) =>
        current.decodeAudioData(bytes),
      );
      entry = { url, buffer };
      cache.set(soundPath, entry);
      const pending = entry;
      void buffer.catch(() => {
        if (cache.get(soundPath) === pending) cache.delete(soundPath);
      });
    }
    const buffer = await entry.buffer;
    // 자산 교체·삭제 이후 완료된 옛 decode 결과는 재생하지 않는다.
    if (disposed || cache.get(soundPath) !== entry) return;
    const source = current.createBufferSource();
    const output = current.createGain();
    source.buffer = buffer;
    output.gain.value = gain;
    source.connect(output);
    output.connect(current.destination);
    const release = () => {
      active.delete(source);
      source.disconnect();
      output.disconnect();
    };
    source.onended = release;
    active.set(source, output);
    try {
      // 매 press마다 독립 source 생성: 앞선 음을 끊지 않는 기존 one-shot 동작.
      source.start();
    } catch (error) {
      release();
      throw error;
    }
  };
  const invalidate = (paths?: readonly string[]) => {
    if (paths) for (const path of paths) cache.delete(path);
    else cache.clear();
  };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    target.removeEventListener('pointerdown', activate, true);
    target.removeEventListener('keydown', activate, true);
    cache.clear();
    for (const [source, output] of active) {
      source.onended = null;
      source.stop();
      source.disconnect();
      output.disconnect();
    }
    active.clear();
    if (context && context.state !== 'closed')
      void context.close().catch((error: unknown) => onError?.(error));
  };
  return { play, unlock, invalidate, dispose };
};
export type WebAudioPreview = ReturnType<typeof createWebAudioPreview>;
